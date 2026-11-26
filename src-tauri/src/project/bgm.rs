use super::model::{Kind, Project};
use super::{Edit, ProjectStore, ProjectView};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Anchor {
    pub scene_id: Uuid,
    // None follows the end even when the scene is extended.
    pub offset_ms: Option<u64>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Clip {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub start: Anchor,
    pub end: Anchor,
    pub volume: u8,
    pub fade_in_ms: u64,
    pub fade_out_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<SourceRange>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceRange {
    pub in_ms: u64,
    pub out_ms: u64,
}

pub(super) fn inventory(project: &Project, project_id: Uuid) -> Value {
    let music: Vec<_> = project.assets.iter().filter(|a| a.kind == Kind::Audio && !a.removed_from_bin)
        .map(|a| json!({"assetId":a.id,"name":a.name,"folderId":a.folder_id,"durationMs":a.duration_ms,"available":!a.missing})).collect();
    json!({"projectId":project_id,"music":music,"clips":project.bgm})
}

pub(super) fn save_clip(
    store: &ProjectStore,
    project: &Project,
    project_id: Uuid,
    expected: Option<&Clip>,
    replacement: Option<Clip>,
) -> Result<ProjectView, String> {
    if store.id != project_id {
        return Err("프로젝트가 바뀌었습니다. BGM 목록을 다시 조회해 주세요.".into());
    }
    let id = replacement
        .as_ref()
        .or(expected)
        .ok_or("편집할 BGM을 지정해 주세요.")?
        .id;
    if expected.is_some_and(|c| c.id != id) {
        return Err("조회한 BGM과 편집할 BGM의 ID가 다릅니다.".into());
    }
    let mut clips = project.bgm.clone();
    let index = clips.iter().position(|c| c.id == id);
    let current = index.map(|i| &clips[i]);
    if current != replacement.as_ref() {
        if current != expected {
            return Err("BGM이 변경되었습니다. 목록을 다시 조회해 주세요.".into());
        }
        match (index, replacement) {
            (Some(i), Some(clip)) => clips[i] = clip,
            (None, Some(clip)) => clips.push(clip),
            (Some(i), None) => {
                clips.remove(i);
            }
            (None, None) => (),
        }
    }
    store.edit(Edit::SetBgm {
        expected: project.bgm.clone(),
        clips,
    })
}

impl Project {
    pub(super) fn anchor_time(&self, anchor: Anchor) -> Option<u64> {
        let mut elapsed = 0u64;
        for scene in &self.scenes {
            let length = scene.settings.duration_ms;
            if scene.id == anchor.scene_id {
                return elapsed.checked_add(anchor.offset_ms.unwrap_or(length).min(length));
            }
            elapsed = elapsed.checked_add(length)?;
        }
        None
    }

    pub(super) fn edit_bgm(
        &mut self,
        expected: Vec<Clip>,
        clips: Vec<Clip>,
    ) -> Result<Option<super::Inverse>, String> {
        if self.bgm != expected {
            return Err("BGM이 변경되었습니다. 다시 시도해 주세요.".into());
        }
        if clips == expected {
            return Ok(None);
        }
        let mut ids = std::collections::HashSet::new();
        for clip in &clips {
            if !ids.insert(clip.id) {
                return Err("중복된 BGM 클립입니다.".into());
            }
            if expected.contains(clip) {
                continue;
            }
            let source = self
                .assets
                .iter()
                .find(|a| a.id == clip.asset_id && a.kind == Kind::Audio)
                .ok_or("BGM 오디오 소스를 찾을 수 없습니다.")?;
            let prior = expected
                .iter()
                .find(|old| old.id == clip.id && old.asset_id == clip.asset_id);
            if prior.is_none() && (source.removed_from_bin || source.missing) {
                return Err("보관함에서 사용할 수 있는 음악을 선택해 주세요.".into());
            }
            if let Some(range) = clip.source_range {
                if range.in_ms >= range.out_ms
                    || source.duration_ms.is_none_or(|ms| range.out_ms > ms)
                {
                    return Err("곡 사용 범위는 원본 음악 길이 안에 있어야 합니다.".into());
                }
            }
            for anchor in [clip.start, clip.end] {
                let scene = &self.scenes[self.scene_index(anchor.scene_id)?];
                if anchor
                    .offset_ms
                    .is_some_and(|ms| ms > scene.settings.duration_ms)
                    && !prior.is_some_and(|old| anchor == old.start || anchor == old.end)
                {
                    return Err("BGM 지점이 지정한 구간 길이를 벗어납니다.".into());
                }
            }
            let start = self
                .anchor_time(clip.start)
                .ok_or("BGM 시작 구간을 찾을 수 없습니다.")?;
            let end = self
                .anchor_time(clip.end)
                .ok_or("BGM 끝 구간을 찾을 수 없습니다.")?;
            if start >= end || clip.volume > 100 {
                return Err("BGM 범위와 음량을 확인해 주세요.".into());
            }
        }
        self.bgm = clips;
        Ok(Some(super::Inverse::Bgm {
            before: expected,
            after: self.bgm.clone(),
        }))
    }

    // Keep surviving coverage when an endpoint scene is deleted. When both
    // endpoints trade places, keep the two boundary scenes in the covered span.
    pub(super) fn reconcile_bgm(&mut self, previous_order: &[Uuid]) -> Option<super::Inverse> {
        let order = super::undo::order(self);
        if order == previous_order || self.bgm.is_empty() {
            return None;
        }
        let before = self.bgm.clone();
        self.bgm.retain_mut(|clip| {
            if !order.contains(&clip.start.scene_id) || !order.contains(&clip.end.scene_id) {
                let first = previous_order
                    .iter()
                    .position(|id| *id == clip.start.scene_id);
                let last = previous_order
                    .iter()
                    .position(|id| *id == clip.end.scene_id);
                let Some(span) = first.zip(last).and_then(|(a, b)| previous_order.get(a..=b))
                else {
                    return false;
                };
                let mut remaining = span.iter().filter(|id| order.contains(id));
                let Some(&first) = remaining.next() else {
                    return false;
                };
                if !order.contains(&clip.start.scene_id) {
                    clip.start = Anchor {
                        scene_id: first,
                        offset_ms: Some(0),
                    };
                }
                if !order.contains(&clip.end.scene_id) {
                    clip.end = Anchor {
                        scene_id: remaining.next_back().copied().unwrap_or(first),
                        offset_ms: None,
                    };
                }
            }
            if order.iter().position(|id| *id == clip.start.scene_id)
                > order.iter().position(|id| *id == clip.end.scene_id)
            {
                std::mem::swap(&mut clip.start.scene_id, &mut clip.end.scene_id);
                clip.start.offset_ms = Some(0);
                clip.end.offset_ms = None;
            }
            true
        });
        (before != self.bgm).then(|| super::Inverse::Bgm {
            before,
            after: self.bgm.clone(),
        })
    }
}

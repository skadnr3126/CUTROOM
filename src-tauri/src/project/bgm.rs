use super::model::{Kind, Project};
use super::{Edit, ProjectStore, ProjectView};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
mod placement;
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
    // Present for single-pass clips. Older projects may intentionally loop music.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_duration_ms: Option<u64>,
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
    mut replacement: Option<Clip>,
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
    if current != replacement.as_ref() && current != expected {
        // A retry may still carry the original, oversized placement request.
        if let Some(clip) = replacement.as_mut() {
            project
                .normalize_bgm_clip(clip, expected.filter(|old| old.asset_id == clip.asset_id))?;
        }
        if current != replacement.as_ref() {
            return Err("BGM이 변경되었습니다. 목록을 다시 조회해 주세요.".into());
        }
    }
    if current != replacement.as_ref() {
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
        mut clips: Vec<Clip>,
    ) -> Result<Option<super::Inverse>, String> {
        if self.bgm != expected {
            return Err("BGM이 변경되었습니다. 다시 시도해 주세요.".into());
        }
        if clips == expected {
            return Ok(None);
        }
        let mut ids = std::collections::HashSet::new();
        for clip in &mut clips {
            if !ids.insert(clip.id) {
                return Err("중복된 BGM 클립입니다.".into());
            }
            if expected.contains(clip) {
                continue;
            }
            let prior = expected
                .iter()
                .find(|old| old.id == clip.id && old.asset_id == clip.asset_id);
            self.normalize_bgm_clip(clip, prior)?;
        }
        if clips == expected {
            return Ok(None);
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

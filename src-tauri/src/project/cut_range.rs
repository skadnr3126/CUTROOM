use super::{
    Edit, ProjectStore, chat_timeline,
    clip_media::{RenderedClip, SourceRange},
    model::{Kind, MIN_DURATION_MS, Project, SceneClip},
    undo::Inverse,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Default)]
pub(super) struct StagedCuts {
    folder: Option<Uuid>,
    clips: Vec<(SourceRange, RenderedClip)>,
}

impl StagedCuts {
    pub(super) fn publish(
        self,
        removed: &[SourceRange],
        project: &mut Project,
        media: &Path,
        added: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        if !self.clips.iter().map(|(range, _)| range).eq(removed) {
            return Err("잘라낼 범위가 변경되었습니다. 다시 선택해 주세요.".into());
        }
        for (_, clip) in self.clips {
            clip.insert(project, media, added, self.folder)?;
        }
        Ok(())
    }
}

impl ProjectStore {
    // Encode outside the project lock. The commit revalidates scene revisions and
    // resolved source ranges before publishing the MP4s and timeline together.
    pub(super) fn stage_cut_sources(&self, edit: &Edit) -> Result<StagedCuts, String> {
        let mut removed = Vec::new();
        let (snapshot, folder) = match edit {
            Edit::CutClipRange {
                scene_id,
                expected,
                range,
                folder_id,
            } => {
                let mut p = self.load()?;
                range.apply(&mut p, *scene_id, *expected, &mut removed)?;
                (p, *folder_id)
            }
            Edit::ApplyChatTimeline {
                job_id,
                base,
                operations,
                folder_id,
            } if operations
                .iter()
                .any(|op| matches!(op, chat_timeline::Operation::CutClipRange { .. })) =>
            {
                let mut p = self.load()?;
                chat_timeline::apply(&mut p, *job_id, base, operations.clone(), &mut removed)?;
                (p, *folder_id)
            }
            _ => return Ok(StagedCuts::default()),
        };
        let mut clips = Vec::new();
        for range in removed {
            let clip = self.render_range(&snapshot, &range)?;
            clips.push((range, clip));
        }
        Ok(StagedCuts { folder, clips })
    }
}

/// Half-open interval measured from the clip's timeline start, including any leading padding.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipRange {
    clip_id: Uuid,
    start_ms: u64,
    end_ms: u64,
}

impl ClipRange {
    fn source_bounds(
        &self,
        project: &Project,
        asset_id: Uuid,
        duration: u64,
        source_in: u64,
        offset: u64,
        source_out: Option<u64>,
    ) -> Result<(u64, u64), String> {
        let asset = project.asset(asset_id)?;
        if asset.kind != Kind::Video
            || self.start_ms <= offset
            || self.start_ms < MIN_DURATION_MS
            || self.start_ms >= self.end_ms
            || duration.saturating_sub(self.end_ms) < MIN_DURATION_MS
        {
            return Err(
                "영상 안에서 시작점과 끝점을 차례로 지정하고 앞뒤에 각각 0.1초 이상 남겨 주세요."
                    .into(),
            );
        }
        let end = source_in
            .checked_add(self.end_ms - offset)
            .ok_or("영상 시간을 확인해 주세요.")?;
        if source_out
            .into_iter()
            .chain(asset.duration_ms)
            .any(|limit| end >= limit)
        {
            return Err("잘라낼 끝점 뒤에 영상이 남아 있어야 합니다.".into());
        }
        Ok((source_in + (self.start_ms - offset), end))
    }

    pub(super) fn apply(
        &self,
        project: &mut Project,
        scene_id: Uuid,
        revision: u32,
        removed: &mut Vec<SourceRange>,
    ) -> Result<Inverse, String> {
        let index = project.scene_index(scene_id)?;
        let before = project.scenes[index].settings.clone();
        let mut settings = before.clone();
        if self.clip_id == scene_id
            && settings.clips.is_empty()
            && let Some(visual) = settings.visual.take()
        {
            settings.clips.push(SceneClip {
                id: scene_id,
                visual,
                duration_ms: settings.duration_ms,
                gap_ms: 0,
            });
        }
        let excerpt;
        if let Some(pos) = settings.clips.iter().position(|c| c.id == self.clip_id) {
            let mut left = settings.clips[pos];
            let placement = left.visual;
            let (cut_in, cut_out) = self.source_bounds(
                project,
                placement.source.asset_id,
                left.duration_ms,
                placement.source.in_ms,
                placement.trim.map_or(0, |t| t.offset_ms),
                placement.trim.map(|t| t.out_ms),
            )?;
            let mut right = left;
            excerpt = SourceRange {
                asset_id: placement.source.asset_id,
                start: cut_in,
                end: cut_out,
            };
            left.duration_ms = self.start_ms;
            right.id = Uuid::new_v4();
            right.duration_ms -= self.end_ms;
            right.gap_ms = 0;
            right.visual.source.in_ms = cut_out;
            if let Some(t) = &mut left.visual.trim {
                t.out_ms = cut_in;
            }
            if let Some(t) = &mut right.visual.trim {
                t.offset_ms = 0;
            }
            // A gap ends the run of clips that moves with this cut.
            if let Some(next) = settings.clips[pos + 1..].iter_mut().find(|c| c.gap_ms > 0) {
                next.gap_ms += self.end_ms - self.start_ms;
            }
            settings.clips.splice(pos..=pos, [left, right]);
        } else {
            for (i, layer) in settings.overlays.iter_mut().enumerate() {
                layer.track.get_or_insert(i as u32 + 1);
            }
            settings.overlays.sort_by_key(|c| (c.track, c.start_ms));
            let pos = settings
                .overlays
                .iter()
                .position(|c| c.id == self.clip_id)
                .ok_or("영상 클립을 찾을 수 없습니다.")?;
            let mut left = settings.overlays[pos];
            let (cut_in, cut_out) = self.source_bounds(
                project,
                left.asset_id,
                left.duration_ms,
                left.in_ms,
                0,
                None,
            )?;
            let mut boundary = left.start_ms + left.duration_ms;
            excerpt = SourceRange {
                asset_id: left.asset_id,
                start: cut_in,
                end: cut_out,
            };
            for c in &mut settings.overlays[pos + 1..] {
                if c.track != left.track || c.start_ms != boundary {
                    break;
                }
                boundary += c.duration_ms;
                c.start_ms -= self.end_ms - self.start_ms;
            }
            let mut right = left;
            left.duration_ms = self.start_ms;
            right.id = Uuid::new_v4();
            right.start_ms += self.start_ms;
            right.in_ms = cut_out;
            right.duration_ms -= self.end_ms;
            settings.overlays.splice(pos..=pos, [left, right]);
            if settings.clips.is_empty() && settings.visual.is_none() {
                settings.duration_ms = settings
                    .overlays
                    .iter()
                    .map(|c| c.start_ms + c.duration_ms)
                    .max()
                    .unwrap_or(0);
            }
        }
        project.edit_scene(scene_id, revision, settings)?;
        removed.push(excerpt);
        Ok(Inverse::Settings {
            id: scene_id,
            before,
            after: project.scenes[index].settings.clone(),
        })
    }
}

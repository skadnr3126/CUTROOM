use super::{
    caption_track::normalize_line_endings,
    model::{Project, SceneClip, SceneSettings},
    undo::Inverse,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum SceneTool {
    DeleteCaption {
        start_ms: u64,
        end_ms: u64,
    },
    AddCaption {
        at_ms: u64,
        text: String,
    },
    RemoveAssTrack,
    EditAssRows {
        edits: Vec<super::ass::Row>,
        additions: Option<Vec<super::ass::NewRow>>,
    },
    SetAssTrack {
        name: String,
        source: String,
    },
    SetCaption {
        text: String,
    },
    SetCaptionCues {
        cues: Vec<super::caption_track::CaptionCue>,
    },
    DuplicateClip {
        clip_id: Uuid,
    },
}

impl SceneTool {
    pub(super) fn apply(
        self,
        project: &mut Project,
        id: Uuid,
        expected: u32,
    ) -> Result<Option<Inverse>, String> {
        let index = project.scene_index(id)?;
        let scene = &mut project.scenes[index];
        if scene.revision != expected {
            return Err("구간이 변경되었습니다. 최신 내용을 확인해 주세요.".into());
        }
        let before = scene.settings.clone();
        match self {
            Self::DeleteCaption { start_ms, end_ms } => {
                if let Some(track) = scene.settings.caption_ass.as_mut() {
                    track.source = super::ass::delete(&track.source, start_ms, end_ms)?;
                } else if let Some(i) = before
                    .caption_cues
                    .iter()
                    .position(|c| (c.start_ms, c.end_ms) == (start_ms, end_ms))
                {
                    scene.settings.caption_cues.remove(i);
                } else if before.caption_cues.is_empty()
                    && !before.caption.trim().is_empty()
                    && (start_ms, end_ms) == (0, before.duration_ms)
                {
                    scene.settings.caption.clear();
                } else {
                    return Err("삭제할 자막을 찾을 수 없습니다.".into());
                }
                scene.revision += 1;
            }
            Self::AddCaption { at_ms, mut text } => {
                normalize_line_endings(&mut text);
                if text.trim().is_empty() || text.contains('\0') || at_ms >= before.duration_ms {
                    return Err("구간 안의 재생 위치와 자막 문구를 확인해 주세요.".into());
                }
                if let Some(track) = scene.settings.caption_ass.as_mut() {
                    track.source =
                        super::ass::insert(&track.source, at_ms, before.duration_ms, &text)?;
                } else {
                    let mut cues = before.caption_cues.clone();
                    if cues.is_empty() && !before.caption.trim().is_empty() {
                        cues.push(super::caption_track::CaptionCue {
                            start_ms: 0,
                            end_ms: before.duration_ms,
                            text: before.caption.clone(),
                        });
                    }
                    super::caption_track::insert(&mut cues, at_ms, before.duration_ms, text)?;
                    scene.settings.caption_cues = cues;
                    scene.settings.caption.clear();
                }
                scene.revision += 1;
            }
            Self::RemoveAssTrack => {
                if scene.settings.caption_ass.take().is_none() {
                    return Ok(None);
                }
                scene.revision += 1;
            }
            Self::EditAssRows { edits, additions } => {
                let track = scene
                    .settings
                    .caption_ass
                    .as_mut()
                    .ok_or("ASS 자막을 먼저 불러와 주세요.")?;
                let source = super::ass::ass_update(
                    track.source.clone(),
                    edits,
                    before.duration_ms,
                    additions,
                )?;
                if source == track.source {
                    return Ok(None);
                }
                track.source = source;
                scene.revision += 1;
            }
            Self::SetAssTrack { name, source } => {
                let track = super::ass::AssTrack { name, source };
                track.validate()?;
                if before.caption_ass.as_ref() == Some(&track) {
                    return Ok(None);
                }
                scene.settings.caption_ass = Some(track);
                scene.settings.caption.clear();
                scene.settings.caption_cues.clear();
                scene.revision += 1;
            }
            Self::SetCaption { .. } | Self::SetCaptionCues { .. }
                if before.caption_ass.is_some() =>
            {
                return Err("ASS 자막 편집을 사용해 주세요.".into());
            }
            Self::SetCaption { mut text } => {
                normalize_line_endings(&mut text);
                if text.contains('\0') {
                    return Err("자막에 사용할 수 없는 문자가 있습니다.".into());
                }
                if !before.caption_cues.is_empty() {
                    return Err("시간 지정 자막은 자막 목록에서 편집해 주세요.".into());
                }
                if before.caption == text {
                    return Ok(None);
                }
                scene.settings.caption = text;
                scene.revision += 1;
            }
            Self::SetCaptionCues { mut cues } => {
                for cue in &mut cues {
                    normalize_line_endings(&mut cue.text);
                }
                super::caption_track::validate(&cues, before.duration_ms)?;
                if before.caption.is_empty() && before.caption_cues == cues {
                    return Ok(None);
                }
                scene.settings.caption.clear();
                scene.settings.caption_cues = cues;
                scene.revision += 1;
            }
            Self::DuplicateClip { clip_id } => {
                let mut settings = before.clone();
                duplicate(&mut settings, id, clip_id)?;
                project.edit_scene(id, expected, settings)?;
            }
        }
        Ok(Some(Inverse::Settings {
            id,
            before,
            after: project.scenes[index].settings.clone(),
        }))
    }
}

fn duplicate(settings: &mut SceneSettings, scene_id: Uuid, clip_id: Uuid) -> Result<(), String> {
    if settings.clips.is_empty()
        && clip_id == scene_id
        && let Some(visual) = settings.visual.take()
    {
        settings.clips.push(SceneClip {
            id: scene_id,
            visual,
            duration_ms: settings.duration_ms,
            gap_ms: 0,
        });
    }
    if let Some(pos) = settings.clips.iter().position(|c| c.id == clip_id) {
        let mut copy = settings.clips[pos];
        copy.id = Uuid::new_v4();
        copy.gap_ms = 0;
        let mut shift = copy.duration_ms;
        for next in &mut settings.clips[pos + 1..] {
            let used = shift.min(next.gap_ms);
            next.gap_ms -= used;
            shift -= used;
            if shift == 0 {
                break;
            }
        }
        settings.clips.insert(pos + 1, copy);
    } else {
        for (i, layer) in settings.overlays.iter_mut().enumerate() {
            layer.track.get_or_insert(i as u32 + 1);
        }
        settings.overlays.sort_by_key(|c| (c.track, c.start_ms));
        let pos = settings
            .overlays
            .iter()
            .position(|c| c.id == clip_id)
            .ok_or("복제할 클립을 찾을 수 없습니다.")?;
        let mut copy = settings.overlays[pos];
        copy.id = Uuid::new_v4();
        copy.start_ms += copy.duration_ms;
        let mut end = copy.start_ms + copy.duration_ms;
        for next in &mut settings.overlays[pos + 1..] {
            if next.track != copy.track || next.start_ms >= end {
                break;
            }
            next.start_ms = end;
            end += next.duration_ms;
        }
        settings.overlays.insert(pos + 1, copy);
    }
    Ok(())
}

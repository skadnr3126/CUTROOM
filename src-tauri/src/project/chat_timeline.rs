use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    model::{
        Kind, Placement, Project, Scene, SceneClip, SceneSettings, VideoTrim, VisualPlacement,
    },
    undo::{self, Inverse},
};


#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct SceneVersion {
    pub id: Uuid,
    pub revision: u32,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum InsertSource {
    Existing(Uuid),
    Rendered(RenderedOutput),
}

#[derive(Clone, Deserialize)]
pub(crate) enum RenderedOutput {
    #[serde(rename = "renderedOutput")]
    ThisRequest,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WritingMode {
    Append,
    Replace,
}

#[derive(Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Operation {
    SceneTool {
        scene_id: Uuid,
        tool: super::scene_tools::SceneTool,
    },
    CutClipRange {
        scene_id: Uuid,
        range: super::cut_range::ClipRange,
        #[serde(default)]
        disposition: super::cut_range::CutDisposition,
    },
    SaveGuidelineStyle {
        expected: u32,
        style: super::guidelines::StyleInput,
    },
    SelectGuidelineStyle {
        previous: Option<Uuid>,
        id: Option<Uuid>,
    },
    SetProductionGuidelines {
        expected: u32,
        text: String,
    },
    RenameScene {
        scene_id: Uuid,
        name: String,
    },
    EditSceneText {
        scene_id: Uuid,
        mode: WritingMode,
        text: String,
    },
    EditSceneNotes {
        scene_id: Uuid,
        mode: WritingMode,
        text: String,
    },
    Insert {
        scene_id: Uuid,
        asset_id: InsertSource,
        position: Option<usize>,
        start_ms: Option<u64>,
        end_ms: Option<u64>,
    },
    Split {
        scene_id: Uuid,
        clip: usize,
        at_ms: u64,
    },
    Trim {
        scene_id: Uuid,
        clip: usize,
        start_ms: u64,
        end_ms: u64,
    },
    RemoveClip {
        scene_id: Uuid,
        clip: usize,
    },
    MoveClip {
        scene_id: Uuid,
        clip: usize,
        position: usize,
    },
    MoveScene {
        scene_id: Uuid,
        position: usize,
    },
}

fn index(position: usize, len: usize) -> Result<usize, String> {
    (1..=len)
        .contains(&position)
        .then(|| position - 1)
        .ok_or_else(|| "타임라인 위치가 범위를 벗어났습니다.".into())
}

pub(super) fn apply(
    project: &mut Project,
    job: Uuid,
    base: &[SceneVersion],
    ops: Vec<Operation>,
    cuts_to_save: &mut Vec<super::clip_media::SourceRange>,
) -> Result<Option<Inverse>, String> {
    if project.timeline_jobs.contains(&job) {
        return Ok(None);
    }
    if base.len() != project.scenes.len()
        || !base
            .iter()
            .zip(&project.scenes)
            .all(|(b, s)| b.id == s.id && b.revision == s.revision)
    {
        return Err(
            "요청 이후 타임라인이 변경되었습니다. 현재 상태에서 다시 요청해 주세요.".into(),
        );
    }
    let mut inverses = Vec::new();
    for op in ops {
        if let Some(inverse) = apply_one(project, job, op, cuts_to_save)? {
            inverses.push(inverse);
        }
    }
    project.timeline_jobs.push(job);
    Ok((!inverses.is_empty()).then(|| Inverse::Group(inverses)))
}

fn apply_one(
    project: &mut Project,
    job: Uuid,
    op: Operation,
    cuts_to_save: &mut Vec<super::clip_media::SourceRange>,
) -> Result<Option<Inverse>, String> {
    let id = match op {
        Operation::SceneTool { scene_id, tool } => {
            let revision = project.scenes[project.scene_index(scene_id)?].revision;
            return tool.apply(project, scene_id, revision);
        }
        Operation::CutClipRange {
            scene_id,
            range,
            disposition,
        } => {
            let revision = project.scenes[project.scene_index(scene_id)?].revision;
            return range
                .apply(project, scene_id, revision, disposition, cuts_to_save)
                .map(Some);
        }
        Operation::SaveGuidelineStyle { expected, style } => {
            return project.save_style(expected, style);
        }
        Operation::SelectGuidelineStyle { previous, id } => {
            if project.selected_style_id != previous {
                return Err("제작 스타일 선택이 변경되었습니다.".into());
            }
            project.select_style(id)?;
            return Ok(None);
        }
        Operation::SetProductionGuidelines { expected, text } => {
            return project.set_production_guidelines(expected, text);
        }
        Operation::RenameScene { scene_id, .. }
        | Operation::EditSceneText { scene_id, .. }
        | Operation::EditSceneNotes { scene_id, .. }
        | Operation::Insert { scene_id, .. }
        | Operation::Split { scene_id, .. }
        | Operation::Trim { scene_id, .. }
        | Operation::RemoveClip { scene_id, .. }
        | Operation::MoveClip { scene_id, .. }
        | Operation::MoveScene { scene_id, .. } => scene_id,
    };
    let scene_index = project.scene_index(id)?;
    let scene = &mut project.scenes[scene_index];
    match op {
        Operation::RenameScene { name, .. } => return rename_scene(scene, name),
        Operation::EditSceneText { mode, text, .. } => {
            return update_writing(scene, |s| &mut s.text, mode, text);
        }
        Operation::EditSceneNotes { mode, text, .. } => {
            return update_writing(scene, |s| &mut s.notes, mode, text);
        }
        _ => {}
    }
    if let Operation::MoveScene { position, .. } = op {
        let target = index(position, project.scenes.len())?;
        if target == scene_index {
            return Ok(None);
        }
        let moved = project.scenes.remove(scene_index);
        project.scenes.insert(target, moved);
        return Ok(Some(Inverse::Move {
            id,
            index: scene_index,
            after: undo::order(project),
        }));
    }
    let scene = &project.scenes[scene_index];
    let rev = scene.revision;
    let before = scene.settings.clone();
    let mut settings = before.clone();
    let mut clips = std::mem::take(&mut settings.clips);
    if clips.is_empty()
        && let Some(visual) = settings.visual.take()
    {
        clips.push(SceneClip {
            gap_ms: 0,
            id,
            visual,
            duration_ms: settings.duration_ms,
        });
    }
    match op {
        Operation::Insert {
            asset_id,
            position,
            start_ms,
            end_ms,
            ..
        } => {
            let asset = match asset_id {
                InsertSource::Existing(id) => project.asset(id)?,
                InsertSource::Rendered(_) => project
                    .assets
                    .iter()
                    .find(|asset| asset.source_job_id == Some(job) && asset.kind == Kind::Video)
                    .ok_or("이번 요청의 완성 영상을 소스 보관함에서 찾을 수 없습니다.")?,
            };
            if asset.removed_from_bin {
                return Err("소스 보관함에서 삭제된 파일입니다.".into());
            }
            let start = start_ms.unwrap_or(0);
            let source = Placement::new(asset.id, start);
            if asset.kind == Kind::Audio {
                if end_ms.is_some() || position.is_some() {
                    return Err("구간 음성 트랙에는 원본 시작점만 지정할 수 있습니다.".into());
                }
                settings = before.clone();
                settings.audio = Some(source);
                return save_settings(project, id, rev, before, settings);
            }
            let pos = index(position.unwrap_or(clips.len() + 1), clips.len() + 1)?;
            let default_end = if asset.kind == Kind::Video {
                asset.duration_ms
            } else {
                Some(if clips.is_empty() {
                    settings.duration_ms
                } else {
                    3000
                })
            };
            let end = end_ms
                .or(default_end)
                .ok_or("길이를 알 수 없는 영상에는 끝점을 지정해 주세요.")?;
            let duration_ms = end
                .checked_sub(start)
                .ok_or("소스 구간 끝점을 확인해 주세요.")?;
            let trim = (asset.kind == Kind::Video).then_some(VideoTrim {
                offset_ms: 0,
                out_ms: end,
            });
            clips.insert(
                pos,
                SceneClip {
                    gap_ms: 0,
                    id: Uuid::new_v4(),
                    duration_ms,
                    visual: VisualPlacement {
                        frame: None,
                        muted: false,
                        source,
                        trim,
                    },
                },
            );
        }
        Operation::RemoveClip { clip, .. } => {
            let idx = index(clip, clips.len())?;
            clips.remove(idx);
        }
        Operation::MoveClip { clip, position, .. } => {
            let from = index(clip, clips.len())?;
            let to = index(position, clips.len())?;
            if from == to {
                return Ok(None);
            }
            let moved = clips.remove(from);
            clips.insert(to, moved);
        }
        Operation::Trim {
            clip,
            start_ms,
            end_ms,
            ..
        } => {
            let idx = index(clip, clips.len())?;
            let c = &mut clips[idx];
            if project.asset(c.visual.source.asset_id)?.kind != Kind::Video {
                return Err("영상 클립의 원본 구간만 자를 수 있습니다.".into());
            }
            c.duration_ms = end_ms
                .checked_sub(start_ms)
                .ok_or("클립 끝점을 확인해 주세요.")?;
            c.visual.source.in_ms = start_ms;
            c.visual.trim = Some(VideoTrim {
                offset_ms: 0,
                out_ms: end_ms,
            });
        }
        Operation::Split { clip, at_ms, .. } => {
            let idx = index(clip, clips.len())?;
            if project.asset(clips[idx].visual.source.asset_id)?.kind != Kind::Video {
                return Err("영상 클립 안쪽의 분할 위치를 지정해 주세요.".into());
            }
            super::split_clip::split_main(project, &mut clips, idx, at_ms)?;
        }
        Operation::SceneTool { .. }
        | Operation::CutClipRange { .. }
        | Operation::SaveGuidelineStyle { .. }
        | Operation::SelectGuidelineStyle { .. }
        | Operation::SetProductionGuidelines { .. }
        | Operation::RenameScene { .. }
        | Operation::EditSceneText { .. }
        | Operation::EditSceneNotes { .. }
        | Operation::MoveScene { .. } => unreachable!(),
    }
    settings.clips = clips;
    save_settings(project, id, rev, before, settings)
}

pub(super) fn rename_scene(scene: &mut Scene, name: String) -> Result<Option<Inverse>, String> {
    update_writing(
        scene,
        |s| &mut s.name,
        WritingMode::Replace,
        super::model::scene_name(&name)?,
    )
}

fn update_writing(
    scene: &mut Scene,
    field: fn(&mut SceneSettings) -> &mut String,
    mode: WritingMode,
    text: String,
) -> Result<Option<Inverse>, String> {
    let current = field(&mut scene.settings);
    let updated = match mode {
        WritingMode::Replace => text,
        WritingMode::Append => {
            if text.trim().is_empty() {
                return Err("추가할 내용이 비어 있습니다.".into());
            }
            let separator =
                if current.is_empty() || current.ends_with('\n') || text.starts_with('\n') {
                    ""
                } else {
                    "\n"
                };
            format!("{current}{separator}{text}")
        }
    };
    if *current == updated {
        return Ok(None);
    }
    let before = scene.settings.clone();
    *field(&mut scene.settings) = updated;
    scene.revision += 1;
    Ok(Some(Inverse::Settings {
        id: scene.id,
        before,
        after: scene.settings.clone(),
    }))
}

fn save_settings(
    project: &mut Project,
    id: Uuid,
    revision: u32,
    before: super::model::SceneSettings,
    settings: super::model::SceneSettings,
) -> Result<Option<Inverse>, String> {
    project.edit_scene(id, revision, settings)?;
    let after = project.scenes[project.scene_index(id)?].settings.clone();
    Ok(Some(Inverse::Settings { id, before, after }))
}

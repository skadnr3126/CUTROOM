//! Local editing commands share the editor's revision checks, history and renderer.
use super::{Edit, ProjectStore, caption_track::CaptionCue, files, render, scene_tools::SceneTool};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Listener};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Command {
    Timeline,
    GetSelection,
    GetAsset {
        asset_id: Uuid,
    },
    Scene {
        scene_id: Uuid,
    },
    RenameScene {
        scene_id: Uuid,
        expected: u32,
        name: String,
    },
    MoveScene {
        scene_id: Uuid,
        target_id: Uuid,
        after: bool,
    },
    Captions {
        scene_id: Uuid,
        expected: u32,
        cues: Vec<CaptionCue>,
        at_ms: Option<u64>,
    },
    AddCaption {
        scene_id: Uuid,
        expected: u32,
        at_ms: u64,
        text: String,
    },
    DeleteCaption {
        scene_id: Uuid,
        expected: u32,
        start_ms: u64,
        end_ms: u64,
    },
    Seek {
        scene_id: Uuid,
        at_ms: u64,
    },
    ImportAss {
        scene_id: Uuid,
        expected: u32,
        path: PathBuf,
    },
    RegisterOutput {
        project_id: Uuid,
        job_id: Uuid,
        path: PathBuf,
        title: Option<String>,
        folder_id: Option<Uuid>,
    },
    ImportVideo {
        path: PathBuf,
    },
    ReplaceVideo {
        scene_id: Uuid,
        expected: u32,
        asset_id: Uuid,
    },
    ExportScene {
        scene_id: Uuid,
        expected: u32,
        path: PathBuf,
    },
}

fn selection(app: &AppHandle, store: &ProjectStore) -> Result<Value, String> {
    let nonce = Uuid::new_v4().to_string();
    let response = format!("cutroom-selection-{nonce}");
    let (send, recv) = std::sync::mpsc::channel();
    let listener = app.listen(response, move |event| {
        let _ = send.send(event.payload().to_owned());
    });
    let result = app
        .emit_to("main", "cutroom-selection-request", nonce)
        .map_err(|e| e.to_string())
        .and_then(|()| {
            recv.recv_timeout(std::time::Duration::from_secs(4))
                .map_err(|_| "편집 화면의 선택 상태를 확인할 수 없습니다.".to_owned())
        });
    app.unlisten(listener);
    let mut value: Value =
        serde_json::from_str(&result?).map_err(|_| "선택 상태를 읽을 수 없습니다.")?;
    if value.as_object_mut().and_then(|v| v.remove("session")) != Some(json!(store.session)) {
        return Err("열린 프로젝트가 바뀌었습니다. 다시 조회해 주세요.".into());
    }
    Ok(value)
}

pub(crate) fn execute(
    app: &AppHandle,
    store: &ProjectStore,
    command: Command,
) -> Result<Value, String> {
    let (mut project, changed) = store.sync_media()?;
    if changed {
        let _ = app.emit("cutroom-project-changed", ());
    }
    let (edited, focus) = match command {
        Command::GetSelection => return selection(app, store),
        Command::GetAsset { asset_id } => {
            let asset = project.asset(asset_id)?;
            let path = asset.path_in(&store.media_dir());
            return Ok(
                json!({"projectId":store.id,"assetId":asset.id,"name":asset.name,"kind":asset.kind,
                "durationMs":asset.duration_ms,"hasAudio":asset.has_audio,"available":path.is_file(),"path":path}),
            );
        }
        Command::Timeline => {
            return Ok(
                json!({"projectId":store.id,"title":project.title,"mediaDirectory":store.media_dir(),"generatedVideoDirectory":store.media_dir(),"folders":project.folders,"scenes":project.scenes.iter().map(|s|
            json!({"id":s.id,"revision":s.revision,"name":s.settings.name,"durationMs":s.settings.duration_ms})).collect::<Vec<_>>()}),
            );
        }
        Command::Scene { scene_id } => {
            let scene = &project.scenes[project.scene_index(scene_id)?];
            return Ok(json!(scene));
        }
        Command::RenameScene {
            scene_id,
            expected,
            name,
        } => {
            let view = store.edit(Edit::RenameScene {
                id: scene_id,
                expected,
                name,
            })?;
            (Some(view), None)
        }
        Command::MoveScene {
            scene_id,
            target_id,
            after,
        } => {
            let view = store.edit(Edit::MoveScene {
                id: scene_id,
                target_id,
                after,
            })?;
            (Some(view), None)
        }
        Command::Seek { scene_id, at_ms } => {
            let scene = &project.scenes[project.scene_index(scene_id)?];
            if at_ms >= scene.settings.duration_ms {
                return Err("재생 위치가 구간 밖입니다.".into());
            }
            (None, Some((scene_id, at_ms)))
        }
        Command::Captions {
            scene_id,
            expected,
            cues,
            at_ms,
        } => {
            let scene = &project.scenes[project.scene_index(scene_id)?];
            if at_ms.is_some_and(|t| t >= scene.settings.duration_ms) {
                return Err("재생 위치가 구간 밖입니다.".into());
            }
            let view = store.edit(Edit::SceneTool {
                scene_id,
                expected,
                tool: SceneTool::SetCaptionCues { cues },
            })?;
            (Some(view), at_ms.map(|t| (scene_id, t)))
        }
        Command::AddCaption {
            scene_id,
            expected,
            at_ms,
            text,
        } => {
            let view = store.edit(Edit::SceneTool {
                scene_id,
                expected,
                tool: SceneTool::AddCaption { at_ms, text },
            })?;
            (Some(view), Some((scene_id, at_ms)))
        }
        Command::DeleteCaption {
            scene_id,
            expected,
            start_ms,
            end_ms,
        } => {
            let view = store.edit(Edit::SceneTool {
                scene_id,
                expected,
                tool: SceneTool::DeleteCaption { start_ms, end_ms },
            })?;
            (Some(view), None)
        }
        Command::ImportAss {
            scene_id,
            expected,
            path,
        } => {
            let track = super::ass_files::read_path(&path)?;
            let view = store.edit(Edit::SceneTool {
                scene_id,
                expected,
                tool: SceneTool::SetAssTrack {
                    name: track.name,
                    source: track.source,
                },
            })?;
            (Some(view), None)
        }
        Command::RegisterOutput {
            project_id,
            job_id,
            path,
            title,
            folder_id,
        } => {
            if project_id != store.id {
                return Err("대상 프로젝트가 일치하지 않습니다.".into());
            }
            let asset = store.register_output(job_id, &path, title.as_deref(), folder_id)?;
            let _ = app.emit("cutroom-project-changed", ());
            return Ok(
                json!({"projectId":store.id,"assetId":asset.id,"title":asset.name,"folderId":asset.folder_id,"path":asset.path_in(&store.media_dir())}),
            );
        }
        Command::ImportVideo { path } => {
            if !path.is_absolute()
                || path
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_none_or(|s| !s.eq_ignore_ascii_case("mp4"))
            {
                return Err("MP4 절대 경로가 필요합니다.".into());
            }
            let probe = files::find_tool(&store.app_data, "ffprobe")
                .ok_or("ffprobe를 찾을 수 없습니다.")?;
            let mut imported = None;
            store.mutate(|p, media, added| {
                if let Some(asset) = super::media_tree::registered_file(p, media, &path)? {
                    imported = Some(asset.id);
                    return Ok(());
                }
                let asset = files::copy_asset(&path, media, Some(&probe))?;
                added.push(asset.path_in(media));
                imported = Some(asset.id);
                p.assets.push(asset);
                Ok(())
            })?;
            let _ = app.emit("cutroom-project-changed", ());
            return Ok(json!({"assetId":imported}));
        }
        Command::ReplaceVideo {
            scene_id,
            expected,
            asset_id,
        } => {
            let mut settings = project.scenes[project.scene_index(scene_id)?]
                .settings
                .clone();
            let asset = project
                .assets
                .iter()
                .find(|a| a.id == asset_id && a.kind == super::model::Kind::Video)
                .ok_or("영상 소스가 없습니다.")?;
            let placement = match settings.clips.as_mut_slice() {
                [] => settings.visual.as_mut().ok_or("교체할 영상이 없습니다.")?,
                [clip] => &mut clip.visual,
                _ => return Err("영상 하나로 된 구간만 교체할 수 있습니다.".into()),
            };
            let old = project
                .assets
                .iter()
                .find(|a| a.id == placement.source.asset_id)
                .ok_or("기존 소스가 없습니다.")?;
            if old.kind != super::model::Kind::Video
                || asset.duration_ms.is_none()
                || old.duration_ms != asset.duration_ms
            {
                return Err("시간을 유지하려면 기존 영상과 같은 길이의 소스가 필요합니다.".into());
            }
            placement.source.asset_id = asset_id;
            (
                Some(store.edit(Edit::UpdateScene {
                    id: scene_id,
                    expected,
                    settings,
                    title_card: None,
                })?),
                None,
            )
        }
        Command::ExportScene {
            scene_id,
            expected,
            path,
        } => {
            let index = project.scene_index(scene_id)?;
            if project.scenes[index].revision != expected {
                return Err("구간이 변경되었습니다.".into());
            }
            if !path.is_absolute()
                || path
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_none_or(|s| !s.eq_ignore_ascii_case("mp4"))
                || path.exists()
            {
                return Err("새 MP4 파일의 절대 경로를 지정하세요.".into());
            }
            project.scenes.retain(|s| s.id == scene_id);
            let ff =
                files::find_tool(&store.app_data, "ffmpeg").ok_or("FFmpeg를 찾을 수 없습니다.")?;
            render::export(&ff, &store.media_dir(), &project, &path)?;
            return Ok(json!({"path":path,"revision":expected}));
        }
    };
    if edited.is_some() {
        let _ = app.emit("cutroom-project-changed", ());
    }
    if let Some((id, t)) = focus {
        app.emit(
            "cutroom-seek",
            json!({"session":store.session,"sceneId":id,"atMs":t}),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(json!({"saved":edited.is_some(),"focus":focus}))
}

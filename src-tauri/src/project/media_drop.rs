use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::Mutex,
};

use serde::Deserialize;
use tauri::{
    State,
    ipc::{InvokeBody, Request},
};
use tempfile::{NamedTempFile, TempPath};
use uuid::Uuid;

use super::drop_tree::{self, DropFolder};
use super::{ProjectStore, ProjectView, files, model::Kind, run_io};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DropFile {
    folder_index: Option<usize>,
    name: String,
    size: u64,
}

struct Incoming {
    source: DropFile,
    file: TempPath,
    written: u64,
}

struct Batch {
    media: std::path::PathBuf,
    id: Uuid,
    files: Vec<Incoming>,
    folders: Vec<DropFolder>,
}

#[derive(Default)]
pub(crate) struct DropInbox(Mutex<Option<Batch>>);

impl Batch {
    fn new(media: &Path, sources: Vec<DropFile>, folders: Vec<DropFolder>) -> Result<Self, String> {
        drop_tree::validate(&folders)?;
        if sources.is_empty() && folders.is_empty() {
            return Err("가져올 파일을 선택해 주세요.".into());
        }
        fs::create_dir_all(media).map_err(|_| "소스 저장 공간을 열 수 없습니다.")?;
        let mut files = Vec::new();
        for source in sources {
            if source
                .folder_index
                .is_some_and(|index| index >= folders.len())
            {
                return Err("소스의 대상 폴더를 찾을 수 없습니다.".into());
            }
            let extension = Path::new(&source.name)
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if source.name.contains(['/', '\\']) || Kind::from_extension(&extension).is_none() {
                return Err(format!("지원하는 소스 형식이 아닙니다. ({})", source.name));
            }
            let file = tempfile::Builder::new()
                .prefix(".drop-")
                .suffix(&format!(".{extension}"))
                .tempfile_in(media)
                .map_err(|_| "소스 복사를 시작할 수 없습니다.")?;
            files.push(Incoming {
                source,
                file: file.into_temp_path(),
                written: 0,
            });
        }
        Ok(Self {
            id: Uuid::new_v4(),
            media: media.to_owned(),
            files,
            folders,
        })
    }

    fn append(&mut self, index: usize, offset: u64, bytes: &[u8]) -> Result<(), String> {
        let file = self
            .files
            .get_mut(index)
            .ok_or("가져올 파일을 찾을 수 없습니다.")?;
        if offset != file.written || bytes.len() as u64 > file.source.size - file.written {
            return Err("파일 전송 위치가 올바르지 않습니다.".into());
        }
        fs::OpenOptions::new()
            .append(true)
            .open(&file.file)
            .and_then(|mut output| output.write_all(bytes))
            .map_err(|_| "소스 파일을 복사할 수 없습니다.")?;
        file.written += bytes.len() as u64;
        Ok(())
    }

    fn import(
        self,
        store: &ProjectStore,
        source_job_id: Option<Uuid>,
        folder_id: Option<Uuid>,
    ) -> Result<ProjectView, String> {
        if self.media != store.media_dir() {
            return Err("다른 프로젝트의 파일 전송입니다.".into());
        }
        let probe = files::find_tool(&store.app_data, "ffprobe");
        store.mutate(|project, media, added| {
            if let Some(id) = source_job_id {
                if project.assets.iter().any(|a| a.source_job_id == Some(id)) {
                    return Ok(());
                }
                if !self.folders.is_empty()
                    || self.files.len() != 1
                    || !self.files[0].source.name.ends_with(".mp4")
                {
                    return Err("영상 생성 결과는 MP4 한 개여야 합니다.".into());
                }
                let mut header = [0; 8];
                let mut input = fs::File::open(&self.files[0].file)
                    .map_err(|_| "렌더링 결과를 열 수 없습니다.")?;
                if input.read_exact(&mut header).is_err() || &header[4..] != b"ftyp" {
                    return Err("렌더링 결과가 MP4 형식인지 확인해 주세요.".into());
                }
            }
            let folder_ids = drop_tree::insert(project, &self.folders, folder_id)?;
            for file in self.files {
                if file.written != file.source.size {
                    return Err(format!(
                        "파일 전송이 완료되지 않았습니다. ({})",
                        file.source.name
                    ));
                }
                let mut asset =
                    files::inspect_asset(&file.file, &file.source.name, probe.as_deref())?;
                asset.folder_id = file
                    .source
                    .folder_index
                    .map(|i| folder_ids[i])
                    .or(folder_id);
                asset.source_job_id = source_job_id;
                let staged = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&file.file)
                    .map_err(|_| {
                        format!("임시 소스 파일을 열 수 없습니다. ({})", file.source.name)
                    })?;
                let staged = NamedTempFile::from_parts(staged, file.file);
                if source_job_id.is_some() {
                    super::generated::publish(
                        project,
                        media,
                        added,
                        asset,
                        super::generated::Input::Staged(staged),
                    )?;
                } else {
                    files::persist_asset(staged, media, &asset)?;
                    added.push(asset.path_in(media));
                    project.assets.push(asset);
                }
            }
            Ok(())
        })
    }
}

#[tauri::command]
pub(crate) fn begin_media_drop(
    store: ProjectStore,
    inbox: State<'_, DropInbox>,
    files: Vec<DropFile>,
    folders: Option<Vec<DropFolder>>,
) -> Result<Uuid, String> {
    let batch = Batch::new(&store.media_dir(), files, folders.unwrap_or_default())?;
    let id = batch.id;
    *inbox
        .0
        .lock()
        .map_err(|_| "가져오기를 시작할 수 없습니다.")? = Some(batch);
    Ok(id)
}

#[tauri::command]
pub(crate) fn append_media_drop(
    inbox: State<'_, DropInbox>,
    request: Request<'_>,
) -> Result<(), String> {
    let header = |key: &str| {
        request
            .headers()
            .get(key)
            .and_then(|v| v.to_str().ok())
            .ok_or("파일 전송 정보를 읽을 수 없습니다.")
    };
    let id: Uuid = header("x-drop-id")?
        .parse()
        .map_err(|_| "파일 전송 ID를 확인해 주세요.")?;
    let index = header("x-file-index")?
        .parse()
        .map_err(|_| "파일 번호를 확인해 주세요.")?;
    let offset = header("x-file-offset")?
        .parse()
        .map_err(|_| "파일 전송 위치를 확인해 주세요.")?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("파일 데이터를 읽을 수 없습니다.".into());
    };
    let mut slot = inbox.0.lock().map_err(|_| "파일을 가져올 수 없습니다.")?;
    slot.as_mut()
        .filter(|b| b.id == id)
        .ok_or("파일 전송이 종료되었습니다.")?
        .append(index, offset, bytes)
}

#[tauri::command]
pub(crate) async fn end_media_drop(
    store: ProjectStore,
    inbox: State<'_, DropInbox>,
    id: Uuid,
    commit: bool,
    source_job_id: Option<Uuid>,
    folder_id: Option<Uuid>,
) -> Result<Option<ProjectView>, String> {
    let batch = inbox
        .0
        .lock()
        .map_err(|_| "파일을 가져올 수 없습니다.")?
        .take_if(|batch| batch.id == id)
        .ok_or("파일 전송이 종료되었습니다.")?;
    run_io(move || {
        if commit {
            batch.import(&store, source_job_id, folder_id).map(Some)
        } else {
            Ok(None)
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Run with CUTROOM_TEST_TOOLS pointing to FFmpeg"]
    fn rendered_command_output_is_validated_and_registered_once() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(temp.path().to_owned());
        let job = Uuid::new_v4();
        let save = |bytes: &[u8]| {
            let mut batch = Batch::new(
                &store.media_dir(),
                vec![DropFile {
                    folder_index: None,
                    name: "효과.mp4".into(),
                    size: bytes.len() as u64,
                }],
                vec![],
            )
            .unwrap();
            batch.append(0, 0, bytes).unwrap();
            batch.import(&store, Some(job), None)
        };
        assert!(save(b"incomplete").is_err());
        assert!(store.load().unwrap().assets.is_empty());
        let tools = std::path::PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap());
        let exe = tools.join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX));
        let output = temp.path().join("output.mp4");
        assert!(
            files::media_command(&exe)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=s=32x32:r=1",
                    "-frames:v",
                    "1"
                ])
                .arg(&output)
                .status()
                .unwrap()
                .success()
        );
        let bytes = fs::read(output).unwrap();
        let first = save(&bytes).unwrap().project.assets[0].id;
        assert_eq!(save(&bytes).unwrap().project.assets.len(), 1);
        // Bind the chat output to the saved asset even though its UUID was unknown to Codex.
        let before = store
            .mutate(|project, _, _| {
                project.add_scene("첫 장면".into());
                project.add_scene("사진 몽타주".into());
                Ok(())
            })
            .unwrap()
            .project;
        let scene = before.scenes[1].id;
        let base: Vec<_> = before
            .scenes
            .iter()
            .map(|s| serde_json::json!({"id":s.id,"revision":s.revision}))
            .collect();
        let place = |request: Uuid, base: &[serde_json::Value]| {
            store.edit(serde_json::from_value(serde_json::json!({
                "type":"applyChatTimeline","jobId":request,"base":base,"operations":[
                    {"type":"insert","sceneId":scene,"assetId":"renderedOutput","position":null,"startMs":null,"endMs":1000}
                ]
            })).unwrap())
        };
        assert!(place(Uuid::new_v4(), &base).is_err());
        assert!(place(job, &[]).is_err());
        let saved = store.load().unwrap();
        assert_eq!(saved.assets.len(), 1);
        assert!(saved.scenes[1].settings.clips.is_empty());
        let view = place(job, &base).unwrap();
        assert!(view.project.scenes[0].settings.clips.is_empty());
        let clips = &view.project.scenes[1].settings.clips;
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].visual.source.asset_id, first);
        assert_eq!(clips[0].duration_ms, 1000);
        assert_eq!(
            place(job, &base).unwrap().project.scenes[1]
                .settings
                .clips
                .len(),
            1
        );
        let token = serde_json::to_value(view.undo.unwrap()).unwrap()["token"].clone();
        let undone = store
            .edit(serde_json::from_value(serde_json::json!({"type":"undo","token":token})).unwrap())
            .unwrap();
        assert!(undone.project.scenes[1].settings.clips.is_empty());
        assert!(
            place(job, &base).unwrap().project.scenes[1]
                .settings
                .clips
                .is_empty()
        );
        let asset = store.load().unwrap().assets.pop().unwrap();
        assert_eq!(asset.id, first);
        assert_eq!(asset.source_job_id, Some(job));
        assert_eq!(fs::read(asset.path_in(&store.media_dir())).unwrap(), bytes);
    }

    #[test]
    fn folder_drop_preserves_tree_and_rolls_back_an_incomplete_copy() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path().to_owned());
        let destination = Uuid::new_v4();
        let existing = Uuid::new_v4();
        for (id, parent, name) in [
            (destination, None, "보관함"),
            (existing, Some(destination), "촬영"),
        ] {
            store
                .edit(super::super::Edit::CreateFolder {
                    id,
                    parent_id: parent,
                    name: name.into(),
                })
                .unwrap();
        }

        let image = include_bytes!("../../icons/32x32.png");
        let batch = || {
            let folders = serde_json::from_value(json!([
                {"name":"촬영"},{"name":"정지 화면","parentIndex":0},{"name":"빈 폴더","parentIndex":1}
            ])).unwrap();
            let files = serde_json::from_value(json!([
                {"name":"같은.png","size":image.len(),"folderIndex":0},
                {"name":"같은.png","size":image.len(),"folderIndex":1},
                {"name":"별도.png","size":image.len()}
            ]))
            .unwrap();
            Batch::new(&store.media_dir(), files, folders).unwrap()
        };
        let mut incomplete = batch();
        incomplete.append(0, 0, image).unwrap();
        assert!(incomplete.import(&store, None, Some(destination)).is_err());
        assert_eq!(store.load().unwrap().folders.len(), 2);
        assert_eq!(fs::read_dir(store.media_dir()).unwrap().count(), 0);

        let mut ready = batch();
        for index in 0..3 {
            ready.append(index, 0, image).unwrap();
        }
        ready.import(&store, None, Some(destination)).unwrap();
        let project = store.load().unwrap();
        let lookup = |name: &str, parent| {
            project
                .folders
                .iter()
                .find(|f| f.name == name && f.parent_id == parent)
                .unwrap()
                .id
        };
        assert_eq!(lookup("촬영", Some(destination)), existing);
        let top = lookup("촬영 (2)", Some(destination));
        let sub = lookup("정지 화면", Some(top));
        lookup("빈 폴더", Some(sub));
        for (asset, parent) in project.assets.iter().zip([top, sub, destination]) {
            assert_eq!(asset.folder_id, Some(parent));
            assert_eq!(fs::read(asset.path_in(&store.media_dir())).unwrap(), image);
        }
        assert_eq!(project.assets.len(), 3);

        let empty = serde_json::from_value(json!([{"name":"빈 보관함"}])).unwrap();
        Batch::new(&store.media_dir(), vec![], empty)
            .unwrap()
            .import(&store, None, None)
            .unwrap();
        let invalid =
            serde_json::from_value(json!([{"name":"잘못된 참조","parentIndex":0}])).unwrap();
        assert!(Batch::new(&store.media_dir(), vec![], invalid).is_err());
    }

    #[test]
    fn dropped_files_commit_together_and_keep_names_and_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(temp.path().to_owned());
        let image = include_bytes!("../../icons/32x32.png");
        let create = || {
            Batch::new(
                &store.media_dir(),
                (0..2)
                    .map(|_| DropFile {
                        folder_index: None,
                        name: "장면.PNG".into(),
                        size: image.len() as u64,
                    })
                    .collect(),
                vec![],
            )
            .unwrap()
        };
        let mut incomplete = create();
        incomplete.append(0, 0, image).unwrap();
        assert!(incomplete.import(&store, None, None).is_err());
        assert!(store.load().unwrap().assets.is_empty());
        assert_eq!(fs::read_dir(store.media_dir()).unwrap().count(), 0);
        let mut batch = create();
        assert!(batch.append(0, 5, &image[..5]).is_err());
        for i in 0..2 {
            batch.append(i, 0, &image[..5]).unwrap();
            batch.append(i, 5, &image[5..]).unwrap();
        }
        let folder = Uuid::new_v4();
        store
            .edit(super::super::Edit::CreateFolder {
                parent_id: None,
                id: folder,
                name: "사진".into(),
            })
            .unwrap();
        let view = batch.import(&store, None, Some(folder)).unwrap();
        assert!(view.project.scenes.is_empty());
        assert_eq!(view.project.assets.len(), 2);
        for asset in store.load().unwrap().assets {
            assert_eq!(asset.folder_id, Some(folder));
            assert_eq!(asset.name, "장면.PNG");
            assert_eq!(fs::read(asset.path_in(&store.media_dir())).unwrap(), image);
        }
    }
}

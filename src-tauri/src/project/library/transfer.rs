use super::{Library, ProjectStore, grant_media, run_io};
use crate::project::{
    asset_filename::filename,
    package::{self, Header, Package},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

#[derive(Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Existing {
    id: Uuid,
    path: PathBuf,
    edited_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preview {
    path: PathBuf,
    header: Header,
    existing: Option<Existing>,
}

fn existing(library: &Library, header: &Header) -> Result<Option<Existing>, String> {
    let mut older_copies = Vec::new();
    for entry in library.recent()? {
        if !entry.path.join("project.json").is_file() {
            continue;
        }
        let exact = package::identity(&entry.path, entry.id)? == header.id;
        if !exact && !header.matches_older_copy(&library.by_id(entry.id)?) {
            continue;
        }
        let candidate = Existing {
            id: entry.id,
            edited_ms: package::edited_ms(&entry.path)?,
            path: entry.path,
        };
        if exact {
            return Ok(Some(candidate));
        }
        older_copies.push(candidate);
    }
    Ok(if older_copies.len() == 1 {
        older_copies.pop()
    } else {
        None
    })
}

#[tauri::command]
pub(crate) async fn project_export_package(
    app: AppHandle,
    store: ProjectStore,
    id: Uuid,
    progress: tauri::ipc::Channel<package::PackageProgress>,
) -> Result<Option<PathBuf>, String> {
    run_io(move || {
        let library = app.state::<Library>();
        if library.current()?.session != store.session {
            return Err("열린 프로젝트가 바뀌었습니다.".into());
        }
        let source = library.by_id(id)?;
        let default_name = format!("{}.cutroom.zip", filename(&source.load()?.title, 1));
        let Some(file) = app
            .dialog()
            .file()
            .set_title("프로젝트 패키지 내보내기")
            .set_file_name(default_name)
            .add_filter("컷룸 패키지", &["zip"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let mut path = file.into_path().map_err(|_| "로컬 파일을 선택해 주세요.")?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("파일 이름을 확인해 주세요.")?;
        if !name.to_ascii_lowercase().ends_with(".cutroom.zip") {
            let stem = if name.to_ascii_lowercase().ends_with(".zip") {
                &name[..name.len() - 4]
            } else {
                name
            };
            path = path.with_file_name(format!("{stem}.cutroom.zip"));
            if path.try_exists().map_err(|e| e.to_string())? {
                return Err(format!(
                    "같은 패키지가 있습니다. 저장 창에서 해당 파일을 직접 선택해 주세요. {}",
                    path.display()
                ));
            }
        }
        package::export(&source, &path, |event| {
            let _ = progress.send(event);
        })?;
        Ok(Some(path))
    })
    .await
}

#[tauri::command]
pub(crate) async fn project_pick_package(
    app: AppHandle,
    _store: ProjectStore,
) -> Result<Option<Preview>, String> {
    run_io(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("가져올 .cutroom.zip 선택")
            .add_filter("컷룸 패키지", &["zip"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|_| "로컬 파일을 선택해 주세요.")?;
        let header = Package::open(&path)?.header;
        let existing = existing(&app.state::<Library>(), &header)?;
        Ok(Some(Preview {
            path,
            header,
            existing,
        }))
    })
    .await
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Mode {
    New,
    Copy,
    Replace,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Edit;
    use std::fs;

    #[test]
    fn replaced_entry_opens_import_and_keeps_previous_folder_as_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let library = Library::open(tmp.path().join("library")).unwrap();
        let source = library
            .activate(
                tmp.path().into(),
                library.current().unwrap().session,
                Some("편집본"),
                |_| Ok(()),
            )
            .unwrap();
        source
            .edit(Edit::AddScene {
                text: Some("원본".into()),
            })
            .unwrap();
        let file = tmp.path().join("편집본.cutroom.zip");
        package::export(&source, &file, |_| {}).unwrap();
        let mut archive = Package::open(&file).unwrap();
        let old = existing(&library, &archive.header).unwrap().unwrap();
        assert_eq!(old.id, source.id);
        let original = fs::read(source.directory().join("project.json")).unwrap();
        let stage = archive.unpack(tmp.path(), false, |_| {}).unwrap();
        let path = package::publish(stage, tmp.path(), "편집본").unwrap();
        let imported = library
            .activate_replacing(path.clone(), source.session, None, Some(old.id), |_| Ok(()))
            .unwrap();
        assert_eq!(
            fs::read(source.directory().join("project.json")).unwrap(),
            original
        );
        assert_eq!(
            existing(&library, &archive.header).unwrap().unwrap().id,
            imported.id
        );
        let reopened = Library::open(tmp.path().join("library")).unwrap();
        assert_eq!(reopened.current().unwrap().directory(), path);
        assert!(reopened.recent().unwrap().iter().all(|e| e.id != old.id));
    }
}

#[tauri::command]
pub(crate) async fn project_import_package(
    app: AppHandle,
    store: ProjectStore,
    preview: Preview,
    mode: Mode,
    progress: tauri::ipc::Channel<package::PackageProgress>,
) -> Result<bool, String> {
    run_io(move || {
        let library = app.state::<Library>();
        let mut archive = Package::open(&preview.path)?;
        if archive.header != preview.header
            || existing(&library, &archive.header)? != preview.existing
        {
            return Err("패키지나 기존 프로젝트가 변경되었습니다. 다시 선택해 주세요.".into());
        }
        if (mode == Mode::New) != preview.existing.is_none() {
            return Err("가져오기 방식을 다시 선택해 주세요.".into());
        }
        let replacing = preview.existing.as_ref().filter(|_| mode == Mode::Replace);
        let parent = if let Some(old) = replacing {
            old.path
                .parent()
                .ok_or("기존 프로젝트의 상위 폴더를 찾을 수 없습니다.")?
                .to_owned()
        } else {
            let Some(folder) = app
                .dialog()
                .file()
                .set_title("프로젝트 폴더를 만들 위치")
                .blocking_pick_folder()
            else {
                return Ok(false);
            };
            folder
                .into_path()
                .map_err(|_| "로컬 폴더를 선택해 주세요.")?
        };
        let parent = dunce::canonicalize(parent).map_err(|e| e.to_string())?;
        for entry in library.recent()? {
            if dunce::canonicalize(&entry.path).is_ok_and(|p| parent.starts_with(p)) {
                return Err("기존 프로젝트 폴더 밖의 위치를 선택해 주세요.".into());
            }
        }
        let stage = archive.unpack(&parent, mode == Mode::Copy, |event| {
            let _ = progress.send(event);
        })?;
        let _lock = if let Some(old) = replacing {
            Some(library.by_id(old.id)?.lock_project()?)
        } else {
            None
        };
        if existing(&library, &archive.header)? != preview.existing {
            return Err("가져오는 동안 기존 프로젝트가 변경되었습니다. 다시 선택해 주세요.".into());
        }
        let _ = progress.send(package::PackageProgress::Opening);
        let path = package::publish(stage, &parent, &archive.header.title)?;
        library
            .activate_replacing(
                path.clone(),
                store.session,
                None,
                replacing.map(|p| p.id),
                |media| grant_media(&app, media),
            )
            .map_err(|e| {
                format!(
                    "{e} 가져온 프로젝트는 다음 폴더에서 열 수 있습니다. {}",
                    path.display()
                )
            })?;
        Ok(true)
    })
    .await
}

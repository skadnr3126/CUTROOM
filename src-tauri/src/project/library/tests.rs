use super::*;
use crate::project::Edit;

#[test]
fn removing_recent_entries_preserves_project_files_and_reopening_restores_id() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("app");
    let library = Library::open(data.clone()).unwrap();
    let legacy = library.current().unwrap();
    let folder = create(temp.path(), "노트북 복사본").unwrap();
    let file = folder.join("project.json");
    fs::write(folder.join("script-draft.json"), "{}").unwrap();
    assert!(manifest_directory(&folder.join("script-draft.json")).is_err());
    let opened = library
        .activate(
            manifest_directory(&file).unwrap(),
            legacy.session,
            None,
            |_| Ok(()),
        )
        .unwrap();
    let content = fs::read(&file).unwrap();
    library.forget(legacy.id, opened.session).unwrap();
    library.forget(opened.id, opened.session).unwrap();
    assert!(library.recent().unwrap().is_empty());
    let next = Library::open(data).unwrap();
    assert!(next.recent().unwrap().is_empty());
    let restored = next
        .activate(
            manifest_directory(&file).unwrap(),
            next.current().unwrap().session,
            None,
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(restored.id, opened.id);
    assert_eq!(next.recent().unwrap().len(), 1);
    assert_eq!(fs::read(file).unwrap(), content);
}

#[test]
fn folder_switches_preserve_legacy_data_and_isolate_writes_and_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("app-data");
    let library = Library::open(data.clone()).unwrap();
    let legacy = library.current().unwrap();
    legacy
        .edit(Edit::AddScene {
            text: Some("기존 구간".into()),
        })
        .unwrap();
    let before = fs::read(legacy.directory().join("project.json")).unwrap();
    let folder = create(tmp.path(), "두 번째 영상").unwrap();
    let second = library
        .activate(folder.clone(), legacy.session, None, |_| Ok(()))
        .unwrap();
    assert!(second.view(second.load().unwrap()).undo.is_none());
    second
        .edit(Edit::AddScene {
            text: Some("두 번째 구간".into()),
        })
        .unwrap();
    assert_eq!(
        before,
        fs::read(legacy.directory().join("project.json")).unwrap()
    );
    assert_ne!(legacy.media_dir(), second.media_dir());
    assert!(create(tmp.path(), "두 번째 영상").is_err());
    assert!(
        library
            .activate(legacy.directory(), legacy.session, None, |_| Ok(()))
            .is_err()
    );
    assert!(
        library
            .activate(tmp.path().into(), second.session, None, |_| Ok(()))
            .is_err()
    );
    assert_eq!(library.current().unwrap().id, second.id);
    let restarted = Library::open(data.clone()).unwrap().current().unwrap();
    assert_eq!(restarted.id, second.id);
    assert_eq!(
        restarted.load().unwrap().scenes[0].settings.text,
        "두 번째 구간"
    );
    let reopened = library
        .activate(legacy.directory(), second.session, None, |_| Ok(()))
        .unwrap();
    assert_ne!(reopened.session, legacy.session);
    assert!(reopened.view(reopened.load().unwrap()).undo.is_some());
    second
        .edit(Edit::Rename {
            title: "지연된 저장".into(),
        })
        .unwrap();
    assert_eq!(
        before,
        fs::read(reopened.directory().join("project.json")).unwrap()
    );
    assert_eq!(library.recent().unwrap().len(), 2);
}

#[test]
fn missing_external_folder_is_never_recreated_on_open() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("app");
    let library = Library::open(data.clone()).unwrap();
    let origin = library.current().unwrap();
    let folder = create(tmp.path(), "원래 위치").unwrap();
    let second = library
        .activate(folder.clone(), origin.session, None, |_| Ok(()))
        .unwrap();
    fs::rename(&folder, tmp.path().join("변경된 위치")).unwrap();
    assert!(second.load().is_err());
    let restored = Library::open(data).unwrap();
    assert!(restored.current().unwrap().id.is_nil());
    assert!(restored.inner.lock().unwrap().startup_notice.is_some());
    assert!(!folder.exists());
}

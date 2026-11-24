use super::*;
use crate::project::{Edit, files};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

#[test]
fn migration_and_external_moves_preserve_scene_source_and_missing_state() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(tmp.path().to_owned());
    let folder = Uuid::new_v4();
    let external = tmp.path().join("아이콘.png");
    let png=STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a0scAAAAASUVORK5CYII=").unwrap();
    fs::write(&external, &png).unwrap();
    let baseline = store
        .commit(|p, media, added| {
            p.create_folder(folder, "스킬".into(), None)?;
            let mut a = files::copy_asset(&external, media, None)?;
            a.folder_id = Some(folder);
            p.add_scene("원래 구간".into());
            p.scenes[0].settings.visual =
                Some(serde_json::from_value(json!({"assetId":a.id,"inMs":0})).unwrap());
            added.push(a.path_in(media));
            p.assets.push(a);
            Ok(())
        })
        .unwrap();
    let id = baseline.assets[0].id;
    let root = store.media_dir();
    let (p, changed) = store.sync_media().unwrap();
    assert!(changed && p.media_tree);
    assert_eq!(p.assets[0].id, id);
    assert_eq!(fs::read(root.join("스킬/아이콘.png")).unwrap(), png);
    assert!(!store.sync_media().unwrap().1);
    fs::rename(root.join("스킬"), root.join("사진")).unwrap();
    fs::rename(root.join("사진/아이콘.png"), root.join("사진/클로.png")).unwrap();
    let (p, _) = store.sync_media().unwrap();
    assert_eq!(p.folders[0].id, folder);
    assert_eq!(p.asset(id).unwrap().name, "클로.png");
    assert_eq!(
        p.asset(id).unwrap().project_file.as_deref(),
        Some("media/사진/클로.png")
    );
    let nested = Uuid::new_v4();
    store
        .edit(Edit::CreateFolder {
            id: nested,
            name: "공중".into(),
            parent_id: Some(folder),
        })
        .unwrap();
    store
        .edit(Edit::MoveSources {
            ids: vec![id],
            folder_id: Some(nested),
        })
        .unwrap();
    store
        .edit(Edit::RenameFolder {
            id: folder,
            name: "아이콘".into(),
        })
        .unwrap();
    let moved = root.join("아이콘/공중/클로.png");
    assert_eq!(fs::read(&moved).unwrap(), png);
    fs::rename(&moved, tmp.path().join("보관.png")).unwrap();
    let (p, _) = store.sync_media().unwrap();
    assert!(p.asset(id).unwrap().missing);
    assert_eq!(p.scenes[0].settings.visual.unwrap().source.asset_id, id);
    fs::rename(tmp.path().join("보관.png"), root.join("다시 찾음.png")).unwrap();
    let (p, _) = store.sync_media().unwrap();
    assert!(!p.asset(id).unwrap().missing);
    assert_eq!(p.asset(id).unwrap().folder_id, None);
    fs::copy(&external, root.join("아이콘/새 파일.png")).unwrap();
    let (p, _) = store.sync_media().unwrap();
    assert_eq!(p.assets.len(), 2);
    assert!(
        p.assets
            .iter()
            .any(|a| a.name == "새 파일.png" && a.folder_id == Some(folder))
    );
    store.edit(Edit::RemoveSource { id }).unwrap();
    assert!(!root.join("다시 찾음.png").exists());
    let (p, _) = store.sync_media().unwrap();
    assert!(p.asset(id).unwrap().path_in(&root).is_file());
    assert_eq!(p.assets.len(), 2);
}

#[test]
fn interrupted_folder_move_is_rolled_back_on_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(tmp.path().to_owned());
    store.sync_media().unwrap();
    fs::create_dir(store.media_dir().join("원본")).unwrap();
    let root = store.directory();
    let mut journal = Journal::new(&root);
    journal.relocate("media/원본", "media/임시").unwrap();
    drop(journal);
    let (p, _) = store.sync_media().unwrap();
    assert!(p.folders.iter().any(|f| f.name == "원본"));
    assert!(!store.media_dir().join("임시").exists());
}

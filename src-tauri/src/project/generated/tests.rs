use super::*;
use crate::project::{Edit, ProjectStore};
use std::fs;

#[test]
fn names_collide_safely_and_renames_preserve_timeline_references() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(temp.path().to_owned());
    store.sync_media().unwrap();
    let insert = |bytes: &[u8]| {
        use std::io::Write;
        let mut id = Uuid::nil();
        store
            .commit(|p, media, added| {
                let mut staged = NamedTempFile::new_in(media).unwrap();
                staged.write_all(bytes).unwrap();
                let a = files::inspect_asset(staged.path(), "클로.mp4", None)?;
                id = publish(p, media, added, a, Input::Staged(staged))?;
                Ok(())
            })
            .unwrap();
        id
    };
    let first = insert(b"first");
    let second = insert(b"second");
    let media = store.media_dir();
    let before = store.load().unwrap();
    assert_ne!(first, second);
    assert_eq!(
        before.asset(second).unwrap().project_file.as_deref(),
        Some("media/클로 (2).mp4")
    );
    let old = before.asset(first).unwrap().path_in(&media);
    store
        .commit(|p, _, _| {
            p.add_scene("클로".into());
            p.scenes[0].settings.visual = Some(
                serde_json::from_value(serde_json::json!({"assetId":first,"inMs":0})).unwrap(),
            );
            Ok(())
        })
        .unwrap();
    store
        .edit(Edit::RenameSource {
            id: first,
            name: "공중 클로".into(),
        })
        .unwrap();
    let saved = store.load().unwrap();
    let asset = saved.asset(first).unwrap();
    assert_eq!(asset.project_file.as_deref(), Some("media/공중 클로.mp4"));
    assert_eq!(
        saved.scenes[0].settings.visual.unwrap().source.asset_id,
        first
    );
    assert!(!old.exists());
    assert_eq!(fs::read(asset.path_in(&media)).unwrap(), b"first");
    assert_eq!(
        fs::read(saved.asset(second).unwrap().path_in(&media)).unwrap(),
        b"second"
    );
    assert_eq!(fs::read_dir(&media).unwrap().count(), 2);
    let failure = store.commit(|p, _, _| {
        p.assets.iter_mut().find(|a| a.id == first).unwrap().name = "실패한 변경".into();
        Err("후속 저장 실패".into())
    });
    assert!(failure.is_err());
    assert!(asset.path_in(&media).exists());
    assert!(!store.directory().join("media/실패한 변경.mp4").exists());
}

#[test]
#[ignore = "Run with CUTROOM_TEST_TOOLS pointing to FFmpeg"]
fn external_job_adopts_a_finished_video_and_retries_once() {
    let temp = tempfile::tempdir().unwrap();
    let tools = PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap());
    let binaries = temp.path().join("tools");
    fs::create_dir(&binaries).unwrap();
    let probe = format!("ffprobe{}", std::env::consts::EXE_SUFFIX);
    if fs::hard_link(tools.join(&probe), binaries.join(&probe)).is_err() {
        fs::copy(tools.join(&probe), binaries.join(probe)).unwrap();
    }
    let store = ProjectStore::new(temp.path().to_owned());
    let output = store.media_dir();
    fs::create_dir_all(&output).unwrap();
    let file = output.join("render.mp4");
    assert!(
        files::media_command(&tools.join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX)))
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
            .arg(&file)
            .status()
            .unwrap()
            .success()
    );
    let job = Uuid::new_v4();
    let asset = store
        .register_output(job, &file, Some("클로"), None)
        .unwrap();
    assert_eq!(asset.project_file.as_deref(), Some("media/클로.mp4"));
    assert_eq!(asset.name, "클로");
    assert!(!file.exists());
    assert_eq!(fs::read_dir(output).unwrap().count(), 1);
    assert_eq!(
        store
            .register_output(job, &file, Some("클로"), None)
            .unwrap()
            .id,
        asset.id
    );
    assert_eq!(store.load().unwrap().assets.len(), 1);
}

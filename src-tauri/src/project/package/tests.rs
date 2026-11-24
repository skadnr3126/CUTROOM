use super::*;
use serde_json::json;

#[test]
fn package_round_trip_keeps_media_ids_captions_draft_and_user_files() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(tmp.path().join("desktop"));
    let root = store.directory();
    let dir = root.join("media/스킬");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir(root.join(".cutroom-retained")).unwrap();
    fs::create_dir(root.join("자막")).unwrap();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let scene = Uuid::new_v4();
    let retained = format!(".cutroom-retained/{second}.mp4");
    let source = json!({"title":"카데나","mediaTree":true,"assets":[
        {"id":first,"name":"평딜","kind":"video","extension":"mp4","durationMs":2000,"hasAudio":true,"projectFile":"media/스킬/평딜.mp4"},
        {"id":second,"name":"마무리","kind":"video","extension":"mp4","durationMs":2000,"hasAudio":false,"removedFromBin":true,"projectFile":retained}
    ],"scenes":[{"id":scene,"revision":4,"name":"마무리","caption":"인사","text":"대본","durationMs":1000,"visual":{"assetId":second,"inMs":0}}]});
    fs::write(root.join("project.json"), source.to_string()).unwrap();
    fs::write(dir.join("평딜.mp4"), vec![42; 2 * 1024 * 1024]).unwrap();
    fs::write(root.join(&retained), b"retained video").unwrap();
    fs::write(root.join("자막/한글.ass"), b"ASS").unwrap();
    fs::write(
        root.join("script-draft.json"),
        br#"{"text":"draft","locations":{}}"#,
    )
    .unwrap();
    let target = tmp.path().join("백업.cutroom.zip");
    export(&store, &target).unwrap();
    let mut archive = Package::open(&target).unwrap();
    assert_eq!(
        archive
            .zip
            .by_name("media/스킬/평딜.mp4")
            .unwrap()
            .compression(),
        CompressionMethod::Stored
    );
    assert!(archive.zip.by_name("project.lock").is_err());
    let lineage = archive.header.id;
    assert_eq!(identity(&root, store.id).unwrap(), lineage);
    let laptop = tmp.path().join("laptop");
    fs::create_dir(&laptop).unwrap();
    fs::create_dir(laptop.join("카데나")).unwrap();
    let mut updates = Vec::new();
    let stage = archive
        .unpack(&laptop, false, |event| updates.push(event))
        .unwrap();
    assert_eq!(updates.pop(), Some(ImportProgress::Verifying));
    let points: Vec<_> = updates
        .iter()
        .map(|event| match event {
            ImportProgress::Extracting {
                percent,
                written_bytes,
                total_bytes,
            } => {
                assert_eq!(u64::from(*percent), written_bytes * 100 / total_bytes);
                (*percent, *written_bytes)
            }
            _ => panic!("unexpected import stage"),
        })
        .collect();
    assert_eq!(points.first(), Some(&(0, 0)));
    assert_eq!(points.last().unwrap().0, 100);
    assert!(points.iter().any(|(p, _)| (20..80).contains(p)));
    assert!(
        points
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0 && pair[0].1 < pair[1].1)
    );
    let opened = publish(stage, &laptop, "카데나").unwrap();
    assert_eq!(opened.file_name().unwrap(), "카데나 (2)");
    let restored = ProjectStore::at(laptop.clone(), opened.clone(), Uuid::new_v4());
    let (project, _) = restored.sync_media().unwrap();
    assert_eq!(project.scenes[0].id, scene);
    assert_eq!(project.scenes[0].settings.caption, "인사");
    assert_eq!(
        project.scenes[0].settings.visual.unwrap().source.asset_id,
        second
    );
    assert_eq!(project.asset(first).unwrap().duration_ms, Some(2000));
    assert_eq!(fs::read(opened.join(&retained)).unwrap(), b"retained video");
    assert_eq!(fs::read(opened.join("자막/한글.ass")).unwrap(), b"ASS");
    assert_eq!(
        fs::read(opened.join("script-draft.json")).unwrap(),
        fs::read(root.join("script-draft.json")).unwrap()
    );
    let back = tmp.path().join("다시.cutroom.zip");
    export(&restored, &back).unwrap();
    assert_eq!(Package::open(&back).unwrap().header.id, lineage);
    let copy = archive.unpack(&laptop, true, |_| {}).unwrap();
    assert_ne!(identity(copy.path(), Uuid::nil()).unwrap(), lineage);
}

#[test]
fn invalid_package_never_publishes_partial_files() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = tmp.path().join("불완전.cutroom.zip");
    let mut writer = ZipWriter::new(File::create(&zip).unwrap());
    let opts = SimpleFileOptions::default();
    writer.start_file(HEADER, opts).unwrap();
    serde_json::to_writer(
        &mut writer,
        &Header {
            version: 1,
            id: Uuid::new_v4(),
            title: "테스트".into(),
            edited_ms: 1,
            source_ids: vec![],
        },
    )
    .unwrap();
    writer.start_file("media/../outside", opts).unwrap();
    writer.write_all(b"bad").unwrap();
    writer.finish().unwrap();
    let parent = tmp.path().join("가져오기");
    fs::create_dir(&parent).unwrap();
    assert!(
        Package::open(&zip)
            .unwrap()
            .unpack(&parent, false, |_| {})
            .is_err()
    );
    assert_eq!(fs::read_dir(parent).unwrap().count(), 0);
    assert!(!tmp.path().join("outside").exists());
}

use super::super::{Edit, ProjectStore, ProjectView, model::SceneSettings};
use serde_json::json;
use tempfile::{TempDir, tempdir};
use uuid::Uuid;

fn id(i: u128) -> Uuid {
    Uuid::from_u128(i)
}

// The main and first overlay tracks both contain an intentional gap. Source files
// are offline: discarding footage needs only the stored timeline placements.
fn fixture() -> (TempDir, ProjectStore, SceneSettings) {
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let video = id(1);
    let image = id(2);
    let view = store.mutate(|project, _, _| {
        for asset in [
            json!({"id":video,"kind":"video","name":"shot","extension":"mp4","durationMs":9000,"hasAudio":true}),
            json!({"id":image,"kind":"image","name":"card","extension":"png","hasAudio":false}),
        ] {
            project.assets.push(serde_json::from_value(asset).unwrap());
        }
        let scene: super::super::model::Scene = serde_json::from_value(json!({
            "id":id(10),"revision":0,"text":"대본","caption":"","durationMs":7100,
            "captionCues":[{"startMs":200,"endMs":1400,"text":"첫 자막"}],
            "audio":null,"visual":null,
            "clips":[
                {"id":id(11),"assetId":video,"inMs":500,"durationMs":2000,"trim":{"offsetMs":200,"outMs":2300},"muted":true},
                {"id":id(12),"assetId":image,"inMs":0,"durationMs":1000},
                {"id":id(13),"assetId":video,"inMs":3000,"durationMs":1800,"gapMs":300,"trim":{"offsetMs":100,"outMs":4700}},
                {"id":id(14),"assetId":video,"inMs":0,"durationMs":600},
                {"id":id(15),"assetId":image,"inMs":0,"durationMs":900,"gapMs":500}
            ],
            "overlays":[
                {"id":id(21),"assetId":video,"track":1,"startMs":100,"inMs":1000,"durationMs":1100,"size":60,"x":12,"y":42,"sound":true},
                {"id":id(22),"assetId":image,"track":1,"startMs":1200,"inMs":0,"durationMs":1000,"size":40,"x":15,"y":35},
                {"id":id(23),"assetId":video,"track":1,"startMs":2200,"inMs":2000,"durationMs":900,"size":62,"x":20,"y":80},
                {"id":id(24),"assetId":image,"track":1,"startMs":3500,"inMs":0,"durationMs":700,"size":100,"x":50,"y":50},
                {"id":id(25),"assetId":image,"track":2,"startMs":800,"inMs":0,"durationMs":4900,"size":25,"x":25,"y":75}
            ]
        })).unwrap();
        project.validate_settings(&scene.settings)?;
        project.scenes.push(scene);
        project.add_scene("다음 구간".into());
        Ok(())
    }).unwrap();
    let before = view.project.scenes[0].settings.clone();
    (dir, store, before)
}

fn delete(
    store: &ProjectStore,
    expected: u32,
    track: u32,
    start_ms: u64,
    end_ms: u64,
) -> Result<ProjectView, String> {
    let edit: Edit = serde_json::from_value(json!({
        "type":"deleteTrackRange","sceneId":id(10),"expected":expected,
        "track":track,"startMs":start_ms,"endMs":end_ms
    }))
    .unwrap();
    store.edit(edit)
}

#[test]
fn main_range_spans_clips_and_a_gap_and_undo_restores_one_edit() {
    let (_dir, store, before) = fixture();
    let view = delete(&store, 0, 0, 1500, 3900).unwrap();
    let scene = &view.project.scenes[0];
    let s = &scene.settings;
    assert_eq!(scene.revision, 1);
    let pieces: Vec<_> = s
        .clips
        .iter()
        .map(|c| (c.id, c.gap_ms, c.duration_ms))
        .collect();
    assert_eq!(
        pieces,
        [
            (id(11), 0, 1500),
            (id(13), 0, 1200),
            (id(14), 0, 600),
            (id(15), 2900, 900)
        ]
    );
    let first = s.clips[0].visual;
    assert_eq!(
        (
            first.source.in_ms,
            first.trim.unwrap().offset_ms,
            first.trim.unwrap().out_ms
        ),
        (500, 200, 1800)
    );
    assert!(first.muted);
    let tail = s.clips[1].visual;
    assert_eq!(
        (
            tail.source.in_ms,
            tail.trim.unwrap().offset_ms,
            tail.trim.unwrap().out_ms
        ),
        (3500, 0, 4700)
    );
    assert!(s.overlays == before.overlays && s.caption_cues == before.caption_cues);
    assert_eq!(s.duration_ms, 7100);
    assert_eq!(view.project.scenes[1].revision, 0);
    assert_eq!(view.project.assets.len(), 2);
    assert!(delete(&store, 0, 0, 1500, 3900).is_err());
    assert!(store.load().unwrap().scenes[0].settings == *s);

    let undo = json!({"type":"undo","token":json!(view.undo.unwrap())["token"]});
    let restored = store.edit(serde_json::from_value(undo).unwrap()).unwrap();
    assert!(restored.project.scenes[0].settings == before);
    assert!(restored.undo.is_none());
}

#[test]
fn overlay_range_preserves_other_tracks_and_stops_at_the_next_gap() {
    let (_dir, store, before) = fixture();
    let view = delete(&store, 0, 1, 600, 2600).unwrap();
    let s = &view.project.scenes[0].settings;
    assert!(s.clips == before.clips);
    assert_eq!(s.duration_ms, before.duration_ms);
    let rows: Vec<_> = s
        .overlays
        .iter()
        .map(|o| (o.id, o.start_ms, o.in_ms, o.duration_ms))
        .collect();
    assert_eq!(
        rows,
        [
            (id(21), 100, 1000, 500),
            (id(23), 600, 2400, 500),
            (id(24), 3500, 0, 700),
            (id(25), 800, 0, 4900)
        ]
    );
    assert_eq!(
        (s.overlays[1].size, s.overlays[1].x, s.overlays[1].y),
        (62, 20, 80)
    );
    assert!(s.overlays[0].sound);
    assert!(s.overlays.last() == before.overlays.last());
}

#[test]
fn cut_in_one_image_keeps_both_pieces_and_their_source() {
    let (_dir, store, _) = fixture();
    let view = delete(&store, 0, 0, 2200, 2600).unwrap();
    let clips = &view.project.scenes[0].settings.clips;
    let (left, right) = (clips[1], clips[2]);
    assert_eq!(left.id, id(12));
    assert_ne!(right.id, left.id);
    assert!(left.visual == right.visual);
    assert_eq!(
        (left.duration_ms, right.duration_ms, right.gap_ms),
        (200, 400, 0)
    );
    assert_eq!((clips[3].id, clips[3].gap_ms), (id(13), 700));
}

#[test]
fn ending_in_a_gap_leaves_the_next_run_at_its_existing_position() {
    let (_dir, store, _) = fixture();
    let view = delete(&store, 0, 0, 2200, 3100).unwrap();
    let clips = &view.project.scenes[0].settings.clips;
    assert_eq!((clips[1].id, clips[1].duration_ms), (id(12), 200));
    assert_eq!((clips[2].id, clips[2].gap_ms), (id(13), 1100));
}

#[test]
fn accepts_exact_edges_and_removing_the_last_clip_preserves_the_scene() {
    let (_dir, store, _) = fixture();
    let view = delete(&store, 0, 0, 2000, 3300).unwrap();
    assert_eq!(view.project.scenes[0].settings.clips[1].id, id(13));
    assert_eq!(view.project.scenes[0].settings.clips[1].gap_ms, 0);
    delete(&store, 1, 0, 0, 7100).unwrap();
    delete(&store, 2, 1, 0, 4200).unwrap();
    let view = delete(&store, 3, 2, 800, 5700).unwrap();
    let scene = &view.project.scenes[0];
    assert!(
        scene.settings.clips.is_empty()
            && scene.settings.visual.is_none()
            && scene.settings.overlays.is_empty()
    );
    assert_eq!(
        (
            scene.id,
            scene.settings.text.as_str(),
            view.project.scenes.len()
        ),
        (id(10), "대본", 2)
    );
}

#[test]
fn invalid_range_leaves_revision_history_and_project_unchanged() {
    let (_dir, store, before) = fixture();
    for (track, start, end) in [
        (0, 900, 900),
        (0, 4000, 2800),
        (0, 0, 7101),
        (9, 500, 2000),
        (0, 3001, 3250),
        (1, 700, 2150),
    ] {
        assert!(
            delete(&store, 0, track, start, end).is_err(),
            "track {track}, {start}–{end}"
        );
    }
    let project = store.load().unwrap();
    assert_eq!(project.scenes[0].revision, 0);
    assert!(project.scenes[0].settings == before);
    assert!(store.history.lock().unwrap().target().is_none());
}

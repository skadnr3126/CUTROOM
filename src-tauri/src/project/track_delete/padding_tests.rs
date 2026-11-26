use super::super::model::{Project, Scene, VideoTrim};
use serde_json::json;
use uuid::Uuid;

fn legacy_scene(source_duration: u64, trim: Option<VideoTrim>) -> (Project, Uuid) {
    let mut project = Project::default();
    let asset = Uuid::new_v4();
    let scene_id = Uuid::new_v4();
    project.assets.push(
        serde_json::from_value(json!({
            "id":asset, "kind":"video", "name":"sample", "extension":"mp4",
            "durationMs":source_duration, "hasAudio":true
        }))
        .unwrap(),
    );
    let scene: Scene = serde_json::from_value(json!({
        "id":scene_id, "revision":0, "durationMs":3000, "text":"", "caption":"", "audio":null,
        "visual":{"assetId":asset, "inMs":500, "trim":trim}
    }))
    .unwrap();
    project.scenes.push(scene);
    (project, scene_id)
}

#[test]
fn all_padding_pieces_keep_their_original_blank_and_visible_windows() {
    let trim = Some(VideoTrim {
        offset_ms: 400,
        out_ms: 2300,
    });
    let (mut project, id) = legacy_scene(4000, trim);
    super::apply(&mut project, id, 0, 0, 200, 1000).unwrap();
    let clips = &project.scenes[0].settings.clips;
    assert_eq!(
        clips[0].visual.trimmed_window(clips[0].duration_ms),
        Some((200, 0))
    );
    assert_eq!(clips[1].visual.source.in_ms, 1100);
    assert_eq!(
        clips[1].visual.trimmed_window(clips[1].duration_ms),
        Some((0, 1200))
    );

    let (mut project, id) = legacy_scene(4000, trim);
    super::apply(&mut project, id, 0, 0, 900, 2400).unwrap();
    let clips = &project.scenes[0].settings.clips;
    assert_eq!(
        clips[0].visual.trimmed_window(clips[0].duration_ms),
        Some((400, 500))
    );
    assert_eq!(
        clips[1].visual.trimmed_window(clips[1].duration_ms),
        Some((600, 0))
    );
}

#[test]
fn deleting_within_untrimmed_last_frame_hold_preserves_original_playback() {
    let (mut project, id) = legacy_scene(2000, None);
    let before = project.scenes[0].settings.clone();
    // Splitting out a standalone still after EOF would not yield a playable source.
    assert!(super::apply(&mut project, id, 0, 0, 1200, 2400).is_err());
    assert!(project.scenes[0].settings == before);
    super::apply(&mut project, id, 0, 0, 1600, 2300).unwrap();
    let clips = &project.scenes[0].settings.clips;
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id, id);
    assert_eq!(clips[0].duration_ms, 2300);
    assert!(clips[0].visual == before.visual.unwrap());
}

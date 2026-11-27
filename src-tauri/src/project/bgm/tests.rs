use super::*;
use crate::project::{Edit, ProjectStore, files, render};
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

fn fixture(store: &ProjectStore) -> (Vec<Uuid>, Uuid) {
    fs::create_dir_all(store.media_dir()).unwrap();
    let song = Uuid::new_v4();
    let view = store.mutate(|p,_,_| {
        p.assets.push(serde_json::from_value(json!({"id":song,"name":"song.wav","kind":"audio","extension":"wav","durationMs":1000,"hasAudio":true})).unwrap());
        for duration in [600, 750, 450] {
            p.add_scene(String::new());
            p.scenes.last_mut().unwrap().settings.duration_ms = duration;
        }
        Ok(())
    }).unwrap();
    let ids: Vec<_> = view.project.scenes.iter().map(|s| s.id).collect();
    let clip = json!({"id":Uuid::new_v4(),"assetId":song,"start":{"sceneId":ids[0],"offsetMs":100},
        "end":{"sceneId":ids[1],"offsetMs":null},"volume":80,"fadeInMs":60,"fadeOutMs":80});
    // Simulate a project saved when a short song could intentionally repeat across scenes.
    store
        .mutate(|p, _, _| {
            p.bgm.push(serde_json::from_value(clip).unwrap());
            Ok(())
        })
        .unwrap();
    (ids, song)
}

#[test]
fn scene_end_survives_resize_reload_delete_and_undo() {
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (ids, _) = fixture(&store);
    let p = store.load().unwrap();
    let mut settings = p.scenes[1].settings.clone();
    settings.duration_ms = 2700;
    store
        .edit(Edit::UpdateScene {
            id: ids[1],
            expected: 0,
            settings,
            title_card: None,
        })
        .unwrap();
    let reopened = ProjectStore::new(dir.path().into()).load().unwrap();
    assert_eq!(reopened.anchor_time(reopened.bgm[0].end), Some(3300));
    assert!(
        store
            .edit(Edit::SetBgm {
                expected: vec![],
                clips: vec![]
            })
            .is_err()
    );
    let removed = store
        .edit(Edit::RemoveScene {
            id: ids[1],
            expected: 1,
        })
        .unwrap();
    assert_eq!(
        removed.project.anchor_time(removed.project.bgm[0].end),
        Some(600)
    );
    let token = serde_json::from_value(json!(removed.undo.unwrap())["token"].clone()).unwrap();
    let undone = store.edit(Edit::Undo { token }).unwrap().project;
    assert_eq!(undone.anchor_time(undone.bgm[0].end), Some(3300));
}

#[test]
#[ignore = "Uses the bundled FFmpeg to verify audible BGM timing"]
fn exports_continuous_music_across_scenes_and_seeks_for_scene_only() {
    let ff = PathBuf::from(env::var_os("CUTROOM_TEST_TOOLS").unwrap())
        .join(format!("ffmpeg{}", env::consts::EXE_SUFFIX));
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (_, song) = fixture(&store);
    let project = store.load().unwrap();
    let path = project.asset(song).unwrap().path_in(&store.media_dir());
    let wav = files::media_command(&ff)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=300:duration=0.5:sample_rate=48000",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=900:duration=0.5:sample_rate=48000",
            "-filter_complex",
            "[0:a][1:a]concat=n=2:v=0:a=1",
            "-f",
            "wav",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        wav.status.success(),
        "{}",
        String::from_utf8_lossy(&wav.stderr)
    );
    let decode = |path: &Path| {
        let samples = files::media_command(&ff)
            .args(["-v", "error", "-i"])
            .arg(path)
            .args([
                "-map", "0:a", "-f", "s16le", "-ar", "8000", "-ac", "1", "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(samples.status.success());
        samples
            .stdout
            .chunks_exact(2)
            .map(|p| i16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>()
    };
    let check_tone = |audio: &[i16], ms: usize, expected: usize| {
        let part = &audio[ms * 8..(ms + 40) * 8];
        if expected == 0 {
            let peak = part.iter().map(|s| s.unsigned_abs()).max().unwrap();
            assert!(peak < 20, "at {ms}ms: expected silence, got peak {peak}");
            return;
        }
        let cycles = part.windows(2).filter(|p| p[0] < 0 && p[1] >= 0).count();
        assert!(
            cycles.abs_diff(expected * 40 / 1000) <= 2,
            "at {ms}ms: {cycles} cycles for {expected} Hz"
        );
    };
    let full = dir.path().join("full.mp4");
    render::export(&ff, &store.media_dir(), &project, &full).unwrap();
    let audio = decode(&full);
    for (ms, hz) in [(15, 0), (320, 300), (800, 900), (1200, 300), (1500, 0)] {
        check_tone(&audio, ms, hz);
    }
    let scene = dir.path().join("scene.mp4");
    render::export_range(&ff, &store.media_dir(), &project, &scene, &[], 1..2).unwrap();
    let audio = decode(&scene);
    check_tone(&audio, 200, 900);
    check_tone(&audio, 600, 300);
    let mut clipped = project;
    clipped.bgm[0].source_range = Some(SourceRange {
        in_ms: 550,
        out_ms: 850,
    });
    let slice = dir.path().join("trimmed-loop.mp4");
    render::export_range(&ff, &store.media_dir(), &clipped, &slice, &[], 1..2).unwrap();
    let audio = decode(&slice);
    check_tone(&audio, 200, 900);
    check_tone(&audio, 600, 900);

    clipped.bgm[0].source_duration_ms = Some(1000);
    let once = dir.path().join("trimmed-once.mp4");
    render::export(&ff, &store.media_dir(), &clipped, &once).unwrap();
    let audio = decode(&once);
    check_tone(&audio, 200, 900);
    let peak = |start: usize| {
        audio[start * 8..(start + 5) * 8]
            .iter()
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap()
    };
    assert!(peak(375) < peak(250) * 3 / 4);
    // The first video frame follows one AAC priming frame (~21 ms) in this mux.
    check_tone(&audio, 430, 0);
    check_tone(&audio, 1000, 0);
}

#[test]
fn mcp_clip_edits_preserve_other_songs_and_reject_stale_changes() {
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    fixture(&store);
    let p = store.load().unwrap();
    let mut second = p.bgm[0].clone();
    second.id = Uuid::new_v4();
    second.source_range = Some(SourceRange {
        in_ms: 200,
        out_ms: 800,
    });
    let add = |p: &Project| save_clip(&store, p, store.id, None, Some(second.clone()));
    let p = add(&p).unwrap().project;
    assert_eq!(add(&p).unwrap().project.bgm.len(), 2);
    let second = p.bgm[1].clone();
    assert_eq!(p.anchor_time(second.end), Some(700));
    assert_eq!(second.source_duration_ms, Some(1000));
    let mut revised = second.clone();
    revised.volume = 0;
    revised.fade_out_ms = 400;
    let p = save_clip(&store, &p, store.id, Some(&second), Some(revised.clone()))
        .unwrap()
        .project;
    assert!(save_clip(&store, &p, store.id, Some(&second), None).is_err());
    assert!(save_clip(&store, &p, Uuid::new_v4(), Some(&revised), None).is_err());
    let mut invalid = revised.clone();
    invalid.source_range = Some(SourceRange {
        in_ms: 400,
        out_ms: 1001,
    });
    assert!(save_clip(&store, &p, store.id, Some(&revised), Some(invalid)).is_err());
    let p = save_clip(&store, &p, store.id, Some(&revised), None)
        .unwrap()
        .project;
    assert_eq!(p.bgm.len(), 1);
    assert_ne!(p.bgm[0].id, second.id);
    assert_eq!(
        inventory(&p, store.id)["music"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn legacy_loops_survive_mix_edits_and_moves_until_resized() {
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (ids, _) = fixture(&store);
    let mut p = store.load().unwrap();
    let mut clip = p.bgm[0].clone();
    clip.volume = 30;
    p.edit_bgm(p.bgm.clone(), vec![clip.clone()]).unwrap();
    assert_eq!(p.bgm[0].source_duration_ms, None);
    clip.start.offset_ms = Some(200);
    clip.end = Anchor {
        scene_id: ids[2],
        offset_ms: Some(100),
    };
    p.edit_bgm(p.bgm.clone(), vec![clip.clone()]).unwrap();
    assert_eq!(p.bgm[0].source_duration_ms, None);

    // Dragging the left edge earlier holds the right edge at 1450 ms.
    clip.start.offset_ms = Some(0);
    p.edit_bgm(p.bgm.clone(), vec![clip]).unwrap();
    assert_eq!(p.bgm[0].source_duration_ms, Some(1000));
    assert_eq!(p.anchor_time(p.bgm[0].start), Some(450));
    assert_eq!(p.anchor_time(p.bgm[0].end), Some(1450));
    let mut changed = p.bgm[0].clone();
    changed.source_duration_ms = None;
    changed.fade_out_ms = 100;
    p.edit_bgm(p.bgm.clone(), vec![changed]).unwrap();
    assert_eq!(p.bgm[0].source_duration_ms, Some(1000));
}

#[test]
fn capped_scene_end_follows_until_source_exhaustion_and_left_trim_pins_visible_end() {
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (ids, _) = fixture(&store);
    let mut p = store.load().unwrap();
    let mut clip = p.bgm[0].clone();
    clip.id = Uuid::new_v4();
    clip.end = Anchor {
        scene_id: ids[0],
        offset_ms: None,
    };
    p.edit_bgm(p.bgm.clone(), vec![clip]).unwrap();
    assert_eq!(p.bgm[0].end.offset_ms, None);
    assert_eq!(p.bgm[0].source_duration_ms, Some(1000));

    p.scenes[0].settings.duration_ms = 2000;
    let clip = p.bgm[0].clone();
    let raw_end = p.anchor_time(clip.end).unwrap();
    assert_eq!(raw_end, 2000);
    assert_eq!(clip.playback_end(100, raw_end, None), 1100);
    let mut oversize = clip.clone();
    oversize.end = Anchor {
        scene_id: ids[1],
        offset_ms: Some(50),
    };
    assert!(p.edit_bgm(p.bgm.clone(), vec![oversize]).unwrap().is_none());
    let mut left_trim = clip;
    left_trim.start.offset_ms = Some(250);
    p.edit_bgm(p.bgm.clone(), vec![left_trim]).unwrap();
    assert_eq!(p.anchor_time(p.bgm[0].start), Some(250));
    assert_eq!(p.anchor_time(p.bgm[0].end), Some(1100));
}

#[test]
fn new_clip_uses_measured_original_length_if_asset_metadata_is_missing() {
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    fixture(&store);
    let mut p = store.load().unwrap();
    p.assets[0].duration_ms = None;
    let mut clip = p.bgm[0].clone();
    clip.id = Uuid::new_v4();
    assert!(p.edit_bgm(p.bgm.clone(), vec![clip.clone()]).is_err());
    clip.source_duration_ms = Some(240);
    p.edit_bgm(p.bgm.clone(), vec![clip]).unwrap();
    assert_eq!(p.anchor_time(p.bgm[0].end), Some(340));
    let mut wrong = p.bgm[0].clone();
    wrong.source_duration_ms = Some(2500);
    wrong.end.offset_ms = Some(580);
    assert!(p.edit_bgm(p.bgm.clone(), vec![wrong]).unwrap().is_none());
}

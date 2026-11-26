use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde_json::Value;
use tempfile::tempdir;

use super::{
    Edit, ProjectStore, files,
    model::{Placement, Project, VideoTrim, VisualPlacement},
    render,
};

fn selected_style(store: &ProjectStore) -> Option<super::guidelines::Style> {
    let project = store.load().unwrap();
    project.guideline_styles.into_iter().find(|s| Some(s.id) == project.selected_style_id)
}

#[test]
fn production_guidelines_survive_reload_and_chat_edits_share_the_same_revision() {
    use serde_json::json;
    use uuid::Uuid;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    fs::create_dir_all(store.directory()).unwrap();
    fs::write(
        store.directory().join("project.json"),
        r#"{"title":"legacy","scenes":[],"assets":[]}"#,
    )
    .unwrap();
    let edit = |store: &ProjectStore, value| store.edit(serde_json::from_value(value).unwrap());
    assert_eq!(
        selected_style(&store).map(|s| s.text).unwrap_or_default(),
        ""
    );
    edit(
        &store,
        json!({"type":"setProductionGuidelines","expected":0,"text":" 세로 영상\n한 줄 자막 "}),
    )
    .unwrap();
    let reopened = ProjectStore::new(dir.path().into());
    let text = selected_style(&reopened).unwrap().text;
    assert_eq!(text, "세로 영상\n한 줄 자막");
    let operation = |expected| {
        json!({"type":"applyChatTimeline","jobId":Uuid::new_v4(),"base":[],
        "operations":[{"type":"setProductionGuidelines","expected":expected,"text":""}]})
    };
    assert!(edit(&reopened, operation(0)).is_err());
    let request = operation(1);
    let cleared = edit(&reopened, request.clone()).unwrap();
    assert_eq!(
        selected_style(&reopened).map(|s| s.text).unwrap_or_default(),
        ""
    );
    assert_eq!(
        edit(&reopened, request)
            .unwrap()
            .project
            .guidelines_revision,
        2
    );
    edit(
        &reopened,
        json!({"type":"undo","token":json!(cleared.undo.unwrap())["token"]}),
    )
    .unwrap();
    assert_eq!(
        selected_style(&reopened).map(|s| s.text).unwrap_or_default(),
        text
    );
}

#[test]
fn styles_migrate_and_preserve_selected_references() {
    use serde_json::json;
    use uuid::Uuid;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    fs::create_dir_all(store.directory()).unwrap();
    let image = Uuid::new_v4();
    let video = Uuid::new_v4();
    fs::write(
        store.directory().join("project.json"),
        json!({
            "title":"legacy","productionGuidelines":"원래 지침","guidelinesRevision":9,"scenes":[],
            "assets":[{"id":image,"name":"사진","extension":"png","kind":"image","hasAudio":false},
                {"id":video,"name":"예시 영상","extension":"mp4","kind":"video","hasAudio":false}]
        })
        .to_string(),
    )
    .unwrap();
    let edit = |value| store.edit(serde_json::from_value(value).unwrap());
    let first = selected_style(&store).unwrap();
    assert_eq!(first.text, "원래 지침");
    let made = edit(
        json!({"type":"saveGuidelineStyle","expected":9,"style":{"id":null,"name":"튜토리얼",
        "text":"차분한 전환","referenceIds":[video,image,video]}}),
    )
    .unwrap();
    let id = made.project.selected_style_id.unwrap();
    let snapshot = selected_style(&ProjectStore::new(dir.path().into())).unwrap();
    assert_eq!(snapshot.id, id);
    assert_eq!(snapshot.reference_ids, vec![video, image]);
    assert_eq!(snapshot.text, "차분한 전환");
    edit(json!({"type":"selectGuidelineStyle","id":null})).unwrap();
    assert!(selected_style(&store).is_none());
    edit(json!({"type":"selectGuidelineStyle","id":Uuid::nil()})).unwrap();
    assert_eq!(
        selected_style(&store).map(|s| s.text).unwrap_or_default(),
        "원래 지침"
    );
    edit(json!({"type":"removeSource","id":video})).unwrap();
    edit(json!({"type":"selectGuidelineStyle","id":id})).unwrap();
    assert_eq!(
        selected_style(&store).unwrap().reference_ids,
        vec![image]
    );
}

fn middle_cut_fixture(store: &ProjectStore) -> (Project, uuid::Uuid, uuid::Uuid, uuid::Uuid) {
    use serde_json::json;
    use uuid::Uuid;
    let scene = Uuid::new_v4();
    let video = Uuid::new_v4();
    let overlay = Uuid::new_v4();
    let baseline = store.mutate(|p, _, _| {
        p.assets.push(serde_json::from_value(json!({"id":video,"kind":"video","name":"shot","extension":"mp4","durationMs":9000,"hasAudio":true})).unwrap());
        p.scenes.push(serde_json::from_value(json!({"id":scene,"revision":0,"text":"대본","caption":"자막","durationMs":4000,"audio":null,
            "visual":{"assetId":video,"inMs":500,"trim":{"offsetMs":200,"outMs":4300},"muted":true},
            "overlays":[{"id":overlay,"assetId":video,"inMs":2500,"durationMs":2000,"startMs":300,"size":50,"x":0,"y":100,"sound":true}]
        })).unwrap());
        Ok(())
    }).unwrap().project;
    (baseline, scene, video, overlay)
}

#[test]
fn splitting_a_clip_keeps_its_frames_and_duration_on_both_tracks() {
    use serde_json::json;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (baseline, scene, _, overlay) = middle_cut_fixture(&store);
    let split = |expected, clip_id, at_ms| {
        store.edit(serde_json::from_value(json!({
            "type":"splitClip","sceneId":scene,"expected":expected,"clipId":clip_id,"atMs":at_ms
        })).unwrap())
    };
    assert!(split(0, scene, 200).is_err()); // At the start of the visible source, within the padded clip.
    let view = split(0, scene, 1000).unwrap();
    let pieces = &view.project.scenes[0].settings.clips;
    assert_eq!((pieces[0].id, pieces[0].duration_ms, pieces[1].duration_ms), (scene, 1000, 3000));
    assert_ne!(pieces[0].id, pieces[1].id);
    assert_eq!(pieces[0].visual.trim.unwrap().out_ms, 1300);
    assert_eq!((pieces[1].visual.source.in_ms, pieces[1].visual.trim.unwrap().offset_ms), (1300, 0));
    assert!(split(0, overlay, 400).is_err());
    let layers = split(1, overlay, 400).unwrap();
    let settings = &layers.project.scenes[0].settings;
    let [left, right] = &settings.overlays[..] else { panic!("expected both overlay pieces") };
    assert_eq!((left.id, left.start_ms, left.duration_ms), (overlay, 300, 400));
    assert_eq!((right.start_ms, right.in_ms, right.duration_ms), (700, 2900, 1600));
    assert_eq!(right.track, left.track);
    assert!(left.sound && right.sound);
    assert_eq!(settings.duration_ms, baseline.scenes[0].settings.duration_ms);
    assert_eq!(layers.project.assets.len(), baseline.assets.len());
    let undo = |view: super::ProjectView| {
        store.edit(serde_json::from_value(json!({"type":"undo","token":json!(view.undo.unwrap())["token"]})).unwrap()).unwrap()
    };
    let restored = undo(undo(layers));
    assert!(restored.project.scenes[0].settings == baseline.scenes[0].settings);
}

#[test]
fn splitting_an_image_preserves_its_display_time() {
    use serde_json::json;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let id = uuid::Uuid::new_v4();
    let asset = uuid::Uuid::new_v4();
    store.mutate(|p, _, _| {
        p.assets.push(serde_json::from_value(json!({"id":asset,"kind":"image","name":"image","extension":"png","hasAudio":false})).unwrap());
        p.scenes.push(serde_json::from_value(json!({"id":id,"revision":0,"text":"","caption":"자막",
            "durationMs":2500,"visual":{"assetId":asset,"inMs":0},"audio":null})).unwrap());
        Ok(())
    }).unwrap();
    let view = store.edit(serde_json::from_value(json!({"type":"splitClip","sceneId":id,"expected":0,"clipId":id,"atMs":1000})).unwrap()).unwrap();
    let settings = &view.project.scenes[0].settings;
    assert_eq!(settings.duration_ms, 2500);
    assert_eq!((settings.clips[0].duration_ms, settings.clips[1].duration_ms), (1000, 1500));
    assert!(settings.clips[0].visual == settings.clips[1].visual);
}

#[test]
fn timed_caption_track_roundtrips_across_clips_chat_and_undo() {
    use serde_json::json;
    use uuid::Uuid;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (_, scene, _, _) = middle_cut_fixture(&store);
    let edit = |value| store.edit(serde_json::from_value(value).unwrap());
    edit(json!({"type":"sceneTool","sceneId":scene,"expected":0,
        "tool":{"type":"duplicateClip","clipId":scene}}))
    .unwrap();
    let cues = json!([{"startMs":500,"endMs":1500,"text":"시작"},{"startMs":3200,"endMs":4600,"text":"경계 통과"}]);
    let set = |expected, list| {
        json!({"type":"sceneTool","sceneId":scene,"expected":expected,
        "tool":{"type":"setCaptionCues","cues":list}})
    };
    edit(set(1, cues.clone())).unwrap();
    assert!(edit(set(1, json!([]))).is_err());
    let invalid =
        json!([{"startMs":100,"endMs":2300,"text":"가"},{"startMs":2200,"endMs":4000,"text":"나"}]);
    assert!(edit(set(2, invalid)).is_err());
    let project = ProjectStore::new(dir.path().into()).load().unwrap();
    let settings = &project.scenes[0].settings;
    assert!(settings.caption.is_empty());
    assert_eq!(json!(settings.caption_cues), cues);
    let pieces = settings.segments();
    assert_eq!(pieces[0].caption_cues[1].end_ms, 4000);
    assert_eq!(pieces[1].caption_cues[0].start_ms, 0);
    assert_eq!(pieces[1].caption_cues[0].end_ms, 600);
    let cleared = edit(json!({"type":"applyChatTimeline","jobId":Uuid::new_v4(),"base":[{"id":scene,"revision":2}],
        "operations":[{"type":"sceneTool","sceneId":scene,"tool":{"type":"setCaptionCues","cues":[]}}]})).unwrap();
    assert!(cleared.project.scenes[0].settings.caption_cues.is_empty());
    let restored =
        edit(json!({"type":"undo","token":json!(cleared.undo.unwrap())["token"]})).unwrap();
    assert_eq!(
        json!(restored.project.scenes[0].settings.caption_cues),
        cues
    );
}

#[test]
fn caption_and_duplicate_share_the_manual_and_chat_edit_paths() {
    use serde_json::json;
    use uuid::Uuid;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (initial, scene, _, overlay) = middle_cut_fixture(&store);
    let edit = |value| store.edit(serde_json::from_value(value).unwrap());
    let caption = |expected, text: &str| {
        json!({
            "type":"sceneTool","sceneId":scene,"expected":expected,"tool":{"type":"setCaption","text":text}
        })
    };
    edit(caption(0, "첫 줄\n둘째 줄")).unwrap();
    assert!(edit(caption(0, "오래된 편집")).is_err());
    assert_eq!(
        edit(caption(1, "첫 줄\n둘째 줄")).unwrap().project.scenes[0].revision,
        1
    );
    let request = json!({"type":"applyChatTimeline","jobId":Uuid::new_v4(),"base":[{"id":scene,"revision":1}],
        "operations":[{"type":"sceneTool","sceneId":scene,"tool":{"type":"setCaption","text":""}},
                      {"type":"sceneTool","sceneId":scene,"tool":{"type":"duplicateClip","clipId":overlay}}]});
    let changed = edit(request.clone()).unwrap();
    edit(request).unwrap();
    let s = &store.load().unwrap().scenes[0].settings;
    assert_eq!(s.caption, "");
    assert_eq!(s.text, initial.scenes[0].settings.text);
    assert_eq!(s.overlays.len(), 2);
    let (original, copy) = (s.overlays[0], s.overlays[1]);
    assert_ne!(original.id, copy.id);
    assert_eq!(copy.start_ms, original.start_ms + original.duration_ms);
    assert_eq!(copy.in_ms, original.in_ms);
    let restored =
        edit(json!({"type":"undo","token":json!(changed.undo.unwrap())["token"]})).unwrap();
    let restored = &restored.project.scenes[0];
    assert_eq!(restored.settings.caption, "첫 줄\n둘째 줄");
    assert_eq!(restored.settings.overlays.len(), 1);
    let manual = edit(
        json!({"type":"sceneTool","sceneId":scene,"expected":restored.revision,
        "tool":{"type":"duplicateClip","clipId":scene}}),
    )
    .unwrap();
    let clips = &manual.project.scenes[0].settings.clips;
    assert_eq!(clips.len(), 2);
    assert_ne!(clips[0].id, clips[1].id);
    assert!(clips[0].visual == clips[1].visual);
    assert_eq!(manual.project.assets.len(), 1);
}

#[test]
fn cut_resolves_source_times_and_missing_media_keeps_the_original_edit() {
    use super::cut_range::CutDisposition;
    use serde_json::json;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (mut baseline, scene, video, overlay) = middle_cut_fixture(&store);
    let initial = baseline.scenes[0].settings.clone();
    let range = json!({"clipId":scene,"startMs":1000,"endMs":2500});
    let edit: Edit = serde_json::from_value(
        json!({"type":"cutClipRange","sceneId":scene,"expected":0,"range":range}),
    )
    .unwrap();
    assert!(store.edit(edit).is_err());
    let stored = store.load().unwrap();
    assert!(stored.scenes[0].settings == initial);
    assert_eq!(stored.assets.len(), 1);
    assert!(store.history.lock().unwrap().target().is_none());
    let mut removed = Vec::new();
    let main: super::cut_range::ClipRange = serde_json::from_value(range).unwrap();
    main.apply(&mut baseline, scene, 0, CutDisposition::Library, &mut removed)
        .unwrap();
    let layer: super::cut_range::ClipRange =
        serde_json::from_value(json!({"clipId":overlay,"startMs":400,"endMs":1200})).unwrap();
    layer.apply(&mut baseline, scene, 1, CutDisposition::Library, &mut removed)
        .unwrap();
    assert_eq!(
        removed
            .iter()
            .map(|r| (r.asset_id, r.start, r.end))
            .collect::<Vec<_>>(),
        vec![(video, 1300, 2800), (video, 2900, 3700)]
    );
}

#[test]
fn cut_discard_only_edits_the_timeline_even_when_source_media_is_offline() {
    use serde_json::json;
    use uuid::Uuid;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (baseline, scene, video, overlay) = middle_cut_fixture(&store);
    let file = baseline.asset(video).unwrap().path_in(&store.media_dir());
    assert!(!file.exists());
    let cut = |clip, expected, start, end, folder| json!({
        "type":"cutClipRange", "disposition":"discard", "folderId":folder,
        "sceneId":scene, "expected":expected, "range":{"clipId":clip,"startMs":start,"endMs":end}
    });
    let edit = |v| store.edit_with_saved_cuts(serde_json::from_value(v).unwrap());
    assert!(edit(cut(scene, 0, 1000, 2500, Some(Uuid::new_v4()))).is_err());
    assert!(edit(cut(scene, 0, 1000, 3999, None)).is_err());
    let (view, saved) = edit(cut(scene, 0, 1000, 2500, None)).unwrap();
    assert!(saved.is_empty());
    let settings = &view.project.scenes[0].settings;
    assert_eq!(settings.duration_ms, 2500);
    assert_eq!(settings.clips.len(), 2);
    assert_eq!(settings.clips[0].visual.trim.unwrap().out_ms, 1300);
    assert_eq!(settings.clips[1].visual.source.in_ms, 2800);
    assert!(settings.overlays == baseline.scenes[0].settings.overlays);
    assert!(edit(cut(overlay, 0, 400, 1200, None)).is_err());
    let (layer, saved) = edit(cut(overlay, 1, 400, 1200, None)).unwrap();
    assert!(saved.is_empty());
    let layers = &layer.project.scenes[0].settings.overlays;
    assert_eq!(layers.len(), 2);
    assert_eq!((layers[0].start_ms, layers[0].duration_ms), (300, 400));
    assert_eq!((layers[1].start_ms, layers[1].in_ms), (700, 3700));
    assert_eq!(layer.project.assets.len(), 1);
    assert_eq!(fs::read_dir(store.media_dir()).unwrap().count(), 0);
    for undo in [layer.undo.unwrap(), view.undo.unwrap()] {
        edit(json!({"type":"undo","token":json!(undo)["token"]})).unwrap();
    }
    assert!(store.load().unwrap().scenes[0].settings == baseline.scenes[0].settings);
}

#[test]
#[ignore = "Run with CUTROOM_TEST_TOOLS pointing to a directory containing FFmpeg and FFprobe"]
fn middle_cut_uses_clip_time_on_both_tracks_and_undo_restores_the_scene() {
    use serde_json::json;
    use uuid::Uuid;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let (baseline, scene, video, overlay) = middle_cut_fixture(&store);
    let tools = PathBuf::from(env::var_os("CUTROOM_TEST_TOOLS").unwrap());
    let local = dir.path().join("tools");
    fs::create_dir(&local).unwrap();
    for tool in ["ffmpeg", "ffprobe"] {
        let name = format!("{tool}{}", env::consts::EXE_SUFFIX);
        fs::hard_link(tools.join(&name), local.join(&name))
            .or_else(|_| fs::copy(tools.join(&name), local.join(&name)).map(|_| ()))
            .unwrap();
    }
    let ffmpeg = files::find_tool(dir.path(), "ffmpeg").unwrap();
    let input = baseline.asset(video).unwrap().path_in(&store.media_dir());
    let encoded = files::media_command(&ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=1280x720:r=20:d=5",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=5",
            "-vf",
            "drawbox=w=iw:h=ih:color=blue:t=fill:enable='between(t,1.3,2.75)'",
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
        ])
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        encoded.status.success(),
        "{}",
        String::from_utf8_lossy(&encoded.stderr)
    );
    let folder = Uuid::new_v4();
    let edit = |v| store.edit(serde_json::from_value(v).unwrap());
    edit(json!({"type":"createFolder","id":folder,"name":"잘라낸 영상","parentId":null})).unwrap();
    let range = |clip, start, end| json!({"clipId":clip,"startMs":start,"endMs":end});
    let manual = |start, end| json!({"type":"cutClipRange","folderId":folder,"sceneId":scene,"expected":0,"range":range(scene,start,end)});
    assert!(edit(manual(100, 1500)).is_err());
    assert!(edit(manual(2000, 500)).is_err());
    let (result, saved_ids) = store
        .edit_with_saved_cuts(serde_json::from_value(manual(1000, 2500)).unwrap())
        .unwrap();
    let saved = &result.project.assets[1];
    assert_eq!(saved_ids, [saved.id]);
    assert_eq!(saved.name, "shot (1.300–2.800초).mp4");
    assert_eq!(saved.folder_id, Some(folder));
    assert!(saved.has_audio && !saved.removed_from_bin);
    assert!((1490..=1550).contains(&saved.duration_ms.unwrap()));
    let pixels = raw_frame(&ffmpeg, &saved.path_in(&store.media_dir()), "0.15");
    assert!(pixels[2] > 190 && pixels[0] < 60);
    let settings = &result.project.scenes[0].settings;
    assert_eq!(settings.duration_ms, 2500);
    assert_eq!(settings.clips[0].visual.trim.unwrap().out_ms, 1300);
    assert_eq!(settings.clips[1].visual.source.in_ms, 2800);
    assert_eq!(settings.clips[1].duration_ms, 1500);
    assert_ne!(settings.clips[0].id, settings.clips[1].id);
    assert!(settings.clips[1].visual.muted);
    assert!(settings.overlays == baseline.scenes[0].settings.overlays);
    assert!(edit(manual(500, 1000)).is_err());
    let restored =
        edit(json!({"type":"undo","token":json!(result.undo.unwrap())["token"]})).unwrap();
    assert!(restored.project.scenes[0].settings == baseline.scenes[0].settings);
    assert!(restored.project.asset(saved.id).is_ok());
    let job = json!({"type":"applyChatTimeline","jobId":Uuid::new_v4(),
        "base":[{"id":scene,"revision":restored.project.scenes[0].revision}],
        "operations":[{"type":"cutClipRange","sceneId":scene,"range":range(scene,1000,2500),"disposition":"discard"},
                      {"type":"cutClipRange","sceneId":scene,"range":range(overlay,400,1200),"disposition":"library"}]});
    let result = edit(job.clone()).unwrap();
    let layer_asset = &result.project.assets[2];
    assert_eq!(layer_asset.name, "shot (2.900–3.700초).mp4");
    assert!(layer_asset.folder_id.is_none());
    let pixels = raw_frame(&ffmpeg, &layer_asset.path_in(&store.media_dir()), "0.15");
    assert!(pixels[0] > 190 && pixels[2] < 60);
    assert_eq!(edit(job).unwrap().project.assets.len(), 3);
    assert_eq!(
        saved.path_in(&store.media_dir()).parent().unwrap(),
        store.media_dir().join("잘라낸 영상")
    );
    assert_eq!(
        layer_asset.path_in(&store.media_dir()).parent().unwrap(),
        store.media_dir()
    );
    let layers = &result.project.scenes[0].settings.overlays;
    assert_eq!(layers.len(), 2);
    assert_eq!((layers[0].start_ms, layers[0].duration_ms), (300, 400));
    assert_eq!(
        (layers[1].start_ms, layers[1].duration_ms, layers[1].in_ms),
        (700, 800, 3700)
    );
    assert_eq!(layers[0].track, layers[1].track);
    assert!(layers[1].sound);
}

#[test]
fn timeline_undo_restores_committed_edits_in_reverse_order() {
    use super::model::{Asset, Kind, SceneClip};
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let asset_id = uuid::Uuid::new_v4();
    let visual = VisualPlacement {
        frame: None,
        muted: false,
        source: Placement {
            asset_id,
            in_ms: 200,
        },
        trim: None,
    };
    let baseline = store
        .mutate(|p, _, _| {
            p.assets.push(Asset {
                disk: None,
                missing: false,
                project_file: None,
                folder_id: None,
                id: asset_id,
                name: "shot".into(),
                extension: "mp4".into(),
                kind: Kind::Video,
                duration_ms: Some(9000),
                has_audio: true,
                source_job_id: None,
                removed_from_bin: false,
            });
            p.add_scene("첫 구간".into());
            p.add_scene("다음 구간".into());
            p.scenes[0].settings.visual = Some(visual);
            p.scenes[0].settings.caption = "자막".into();
            Ok(())
        })
        .unwrap()
        .project;
    let first = &baseline.scenes[0];
    let second = baseline.scenes[1].id;
    let mut split = first.settings.clone();
    split.visual = None;
    split.clips = vec![
        SceneClip {
            gap_ms: 0,
            id: first.id,
            duration_ms: 1500,
            visual,
        },
        SceneClip {
            gap_ms: 0,
            id: uuid::Uuid::new_v4(),
            duration_ms: 2500,
            visual: VisualPlacement {
                frame: None,
                muted: false,
                source: Placement {
                    asset_id,
                    in_ms: 1700,
                },
                trim: None,
            },
        },
    ];
    let update = |expected, settings| {
        store
            .edit(Edit::UpdateScene {
                id: first.id,
                expected,
                settings,
                title_card: None,
            })
            .unwrap()
    };
    let token = |v: &super::ProjectView| {
        serde_json::to_value(v.undo).unwrap()["token"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    };
    let split_view = update(0, split.clone());
    let mut swapped = split.clone();
    swapped.clips.reverse();
    let swap_view = update(1, swapped.clone());
    let mut deleted = swapped.clone();
    deleted.clips.pop();
    let delete_view = update(2, deleted);
    let moved = store
        .edit(Edit::MoveScene {
            id: first.id,
            target_id: second,
            after: true,
        })
        .unwrap();
    store
        .edit(Edit::RenameSource {
            id: asset_id,
            name: "renamed".into(),
        })
        .unwrap();
    let undo = |v: &super::ProjectView| store.edit(Edit::Undo { token: token(v) });
    assert!(undo(&split_view).is_err());
    let restored_order = undo(&moved).unwrap();
    assert_eq!(restored_order.project.scenes[0].id, first.id);
    assert!(undo(&moved).is_err());
    assert!(undo(&delete_view).unwrap().project.scenes[0].settings == swapped);
    assert!(undo(&swap_view).unwrap().project.scenes[0].settings == split);
    let done = undo(&split_view).unwrap();
    assert!(done.undo.is_none());
    let saved = store.load().unwrap();
    assert!(saved.scenes[0].settings == first.settings);
    assert!(saved.scenes[0].revision > 3);
    assert_eq!(saved.assets[0].name, "renamed");

    let removed = store
        .edit(Edit::RemoveScene {
            id: first.id,
            expected: saved.scenes[0].revision,
        })
        .unwrap();
    assert!(undo(&removed).unwrap().project.scenes[0].settings == first.settings);
    let added = store
        .edit(Edit::AddScene {
            text: Some("새 구간".into()),
        })
        .unwrap();
    assert_eq!(undo(&added).unwrap().project.scenes.len(), 2);
}

#[test]
fn moving_a_scene_uses_the_target_boundary_and_preserves_other_scenes() {
    let temp = tempdir().unwrap();
    let store = ProjectStore::new(temp.path().into());
    let initial = store
        .mutate(|project, _, _| {
            for text in ["A", "B", "C", "D"] {
                project.add_scene(text.into());
            }
            Ok(())
        })
        .unwrap()
        .project;
    let ids: Vec<_> = initial.scenes.iter().map(|scene| scene.id).collect();
    let move_to = |id: uuid::Uuid, target_id: uuid::Uuid, after: bool| {
        store.edit(
            serde_json::from_value(serde_json::json!({
                "type": "moveScene", "id": id, "targetId": target_id, "after": after
            }))
            .unwrap(),
        )
    };
    let order = || {
        store
            .load()
            .unwrap()
            .scenes
            .into_iter()
            .map(|scene| scene.id)
            .collect::<Vec<_>>()
    };
    move_to(ids[0], ids[2], true).unwrap();
    assert_eq!(order(), [ids[1], ids[2], ids[0], ids[3]]);
    move_to(ids[3], ids[1], false).unwrap();
    let expected = [ids[3], ids[1], ids[2], ids[0]];
    assert_eq!(order(), expected);
    assert!(move_to(ids[0], uuid::Uuid::new_v4(), false).is_err());
    assert_eq!(order(), expected);
}

#[test]
fn script_links_survive_reorder_and_save_and_stale_edits_are_rejected() {
    let temp = tempdir().unwrap();
    let store = ProjectStore::new(temp.path().to_owned());
    let selected = "첫 구간\n연속 문장\n\n같은 구간의 다음 문단";
    let first = store
        .edit(Edit::AddScene {
            text: Some(selected.into()),
        })
        .unwrap()
        .project;
    assert_eq!(first.scenes.len(), 1);
    assert_eq!(first.scenes[0].settings.text, selected);
    assert!(
        store
            .edit(Edit::AddScene {
                text: Some("\n  ".into())
            })
            .is_err()
    );
    let initial = store
        .edit(Edit::AddScene {
            text: Some("둘째 구간".into()),
        })
        .unwrap()
        .project;
    assert_eq!(initial.scenes.len(), 2);
    assert_eq!(initial.scenes[0].settings.text, selected);
    let id = initial.scenes[0].id;
    let source = temp.path().join("촬영본.mp4");
    fs::write(&source, b"fixture").unwrap();
    let with_source = store
        .mutate(|project, media, added| {
            let asset = files::copy_asset(&source, media, None)?;
            added.push(asset.path_in(media));
            project.assets.push(asset);
            Ok(())
        })
        .unwrap()
        .project;
    let source_id = with_source.assets[0].id;
    let mut settings = initial.scenes[0].settings.clone();
    settings.visual = Some(VisualPlacement {
        frame: None,
        muted: false,
        source: Placement {
            asset_id: source_id,
            in_ms: 250,
        },
        trim: None,
    });
    settings.caption = "한글 자막".into();
    settings.notes = "장면 전환 전 1초 정지".into();
    settings.name = "도입부".into();
    store
        .edit(Edit::UpdateScene {
            id,
            expected: 0,
            settings: settings.clone(),
            title_card: None,
        })
        .unwrap();
    store
        .edit(Edit::MoveScene {
            id,
            target_id: initial.scenes[1].id,
            after: true,
        })
        .unwrap();
    let reopened = ProjectStore::new(temp.path().to_owned());
    let current = reopened.load().unwrap();
    assert_eq!(current.scenes[1].id, id);
    assert_eq!(
        current.scenes[1].settings.visual.unwrap().source.asset_id,
        source_id
    );
    assert_eq!(current.scenes[1].settings.caption, "한글 자막");
    assert_eq!(
        fs::read(current.assets[0].path_in(&reopened.media_dir())).unwrap(),
        b"fixture"
    );
    assert_eq!(current.scenes[1].settings.notes, "장면 전환 전 1초 정지");
    assert_eq!(current.scenes[1].settings.name, "도입부");
    assert_eq!(current.scenes[1].revision, 1);
    assert_eq!(current.assets[0].name, "촬영본.mp4");

    settings.caption = "낡은 요청".to_owned();
    assert!(
        reopened
            .edit(Edit::UpdateScene {
                id,
                expected: 0,
                settings,
                title_card: Some("iVBORw0KGgo=".to_owned()),
            })
            .is_err()
    );
    assert_eq!(
        reopened.load().unwrap().scenes[1].settings.caption,
        "한글 자막"
    );
    assert_eq!(fs::read_dir(reopened.media_dir()).unwrap().count(), 1);
}

#[test]
#[ignore = "Run with CUTROOM_TEST_TOOLS pointing to a directory containing FFmpeg and FFprobe"]
fn renders_trimmed_clips_with_korean_captions_and_voice() {
    let tools = PathBuf::from(env::var_os("CUTROOM_TEST_TOOLS").expect("FFmpeg tool directory"));
    let ffmpeg = tools.join(format!("ffmpeg{}", env::consts::EXE_SUFFIX));
    let ffprobe = tools.join(format!("ffprobe{}", env::consts::EXE_SUFFIX));
    let temp = tempdir().unwrap();
    let store = ProjectStore::new(temp.path().to_owned());
    let video = temp.path().join("원본.mp4");
    let image = temp.path().join("타이틀.png");
    let voice = temp.path().join("음성.wav");
    for (args, path) in [
        (
            vec![
                "-f",
                "lavfi",
                "-i",
                "color=blue:s=320x180:r=30:d=1.2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1.2",
                "-vf",
                "drawbox=w=iw:h=ih:color=red:t=fill:enable='lt(t,0.3)+gte(t,0.9)'",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                "-shortest",
            ],
            &video,
        ),
        (
            vec![
                "-f",
                "lavfi",
                "-i",
                "color=green:s=640x360,drawbox=x=100:y=80:w=40:h=80:color=red:t=fill",
                "-frames:v",
                "1",
                "-c:v",
                "png",
            ],
            &image,
        ),
        (
            vec!["-f", "lavfi", "-i", "sine=frequency=700:duration=2.5"],
            &voice,
        ),
    ] {
        let output = files::media_command(&ffmpeg)
            .args(["-nostdin", "-v", "error"])
            .args(args)
            .args(["-threads", "1"])
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let view = store
        .mutate(|project, media, added| {
            for path in [&video, &image, &voice] {
                let asset = files::copy_asset(path, media, Some(&ffprobe))?;
                added.push(asset.path_in(media));
                project.assets.push(asset);
            }
            project.add_scene("첫 장면".into());
            project.add_scene("두 번째 장면".into());
            let first = &mut project.scenes[0].settings;
            first.duration_ms = 1500;
            first.visual = Some(VisualPlacement {
                frame: None,
                muted: false,
                source: Placement {
                    asset_id: project.assets[0].id,
                    in_ms: 300,
                },
                trim: Some(VideoTrim {
                    offset_ms: 300,
                    out_ms: 900,
                }),
            });
            first.caption = "한글 자막 100%".to_owned();
            let second = &mut project.scenes[1].settings;
            second.duration_ms = 700;
            second.visual = Some(VisualPlacement {
                frame: None,
                muted: false,
                source: Placement {
                    asset_id: project.assets[1].id,
                    in_ms: 0,
                },
                trim: None,
            });
            second.caption_cues = serde_json::from_value(serde_json::json!([
                {"startMs":100,"endMs":300,"text":"첫 자막"},
                {"startMs":400,"endMs":650,"text":"두 번째 자막"}
            ]))
            .unwrap();
            second.audio = Some(Placement {
                asset_id: project.assets[2].id,
                in_ms: 250,
            });
            Ok(())
        })
        .unwrap();
    assert!(view.project.assets[0].duration_ms.unwrap() >= 1200);
    let restored = store.load().unwrap();
    let scene = &restored.scenes[0].settings;
    assert_eq!(
        scene.visual.unwrap().trimmed_window(scene.duration_ms),
        Some((300, 600))
    );
    let mut invalid = scene.clone();
    invalid
        .visual
        .as_mut()
        .unwrap()
        .trim
        .as_mut()
        .unwrap()
        .out_ms = 100_000;
    assert!(restored.validate_settings(&invalid).is_err());
    let destination = temp.path().join("편집 결과.mp4");
    render::export(&ffmpeg, &store.media_dir(), &view.project, &destination).unwrap();
    let probe = files::media_command(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(&destination)
        .output()
        .unwrap();
    assert!(probe.status.success());
    let output: Value = serde_json::from_slice(&probe.stdout).unwrap();
    assert_eq!(output["streams"][0]["codec_name"], "h264");
    assert_eq!(output["streams"][0]["width"], 1280);
    assert_eq!(output["streams"][1]["codec_name"], "aac");
    let duration: f64 = output["format"]["duration"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((duration - 2.2).abs() < 0.12, "duration was {duration}");
    let frame = raw_frame(&ffmpeg, &destination, "0.5");
    let center = (1280 * 200 + 640) * 3;
    assert!(
        frame[center + 2] > 190 && frame[center] < 60,
        "source range should start on the blue section"
    );
    for time in ["0.1", "1.1"] {
        let outside = raw_frame(&ffmpeg, &destination, time);
        assert!(
            outside[center..center + 3].iter().all(|value| *value < 48),
            "video leaked outside the range at {time}"
        );
    }
    for (time, visible) in [
        ("1.55", false),
        ("1.70", true),
        ("1.85", false),
        ("2.00", true),
    ] {
        let frame = raw_frame(&ffmpeg, &destination, time);
        let white = frame[1280 * 580 * 3..]
            .chunks_exact(3)
            .filter(|rgb| rgb.iter().all(|v| *v > 200))
            .count();
        assert_eq!(
            white > 50,
            visible,
            "자막 표시 시각 {time}, 흰 픽셀 {white}"
        );
    }
    let samples = files::media_command(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&destination)
        .args(["-vn", "-ar", "48000", "-ac", "2", "-f", "s16le", "-"])
        .output()
        .unwrap();
    assert!(samples.status.success());
    let bytes_per_ms = 48 * 2 * 2;
    for (time, expected) in [(100, false), (500, true), (1100, false), (1700, true)] {
        let peak = samples.stdout[time * bytes_per_ms..(time + 100) * bytes_per_ms]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|sample| i16::from_le_bytes(*sample).unsigned_abs())
            .max()
            .unwrap();
        assert_eq!(
            peak > 100,
            expected,
            "unexpected audio at {time}ms, peak {peak}"
        );
    }
    let subtitle_pixels = frame
        .as_chunks::<3>()
        .0
        .iter()
        .skip(1280 * 570)
        .filter(|pixel| pixel.iter().all(|channel| *channel > 220))
        .count();
    assert!(
        subtitle_pixels > 100,
        "no light caption pixels over the blue clip"
    );

    let tool_dir = temp.path().join("tools");
    fs::create_dir(&tool_dir).unwrap();
    for executable in [&ffmpeg, &ffprobe] {
        let link = tool_dir.join(executable.file_name().unwrap());
        if fs::hard_link(executable, &link).is_err() {
            fs::copy(executable, link).unwrap();
        }
    }
    let ids: Vec<_> = view.project.assets.iter().map(|asset| asset.id).collect();
    let job_id = uuid::Uuid::new_v4();
    let recipe = serde_json::json!({
        "jobId":job_id, "assetIds":ids, "name":"채팅 편집본",
        "clips":[
            {"durationMs":500,"visualAssetId":ids[0],"visualInMs":300,"audioAssetId":null,"audioInMs":0,"caption":""},
            {"durationMs":600,"visualAssetId":ids[1],"visualInMs":0,"audioAssetId":ids[2],"audioInMs":200,"caption":"마무리"}
        ]
    });
    let mut invalid = recipe.clone();
    invalid["assetIds"] = serde_json::json!([ids[1]]);
    assert!(
        store
            .render_source(serde_json::from_value(invalid).unwrap())
            .is_err()
    );
    let result = store
        .render_source(serde_json::from_value(recipe.clone()).unwrap())
        .unwrap();
    let repeated = store
        .render_source(serde_json::from_value(recipe).unwrap())
        .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap()["assetId"],
        serde_json::to_value(repeated).unwrap()["assetId"]
    );
    let reopened = store.load().unwrap();
    assert_eq!(reopened.scenes.len(), 2);
    assert_eq!(reopened.scenes[0].settings.duration_ms, 1500);
    assert_eq!(reopened.assets.len(), 4);
    let output = &reopened.assets[3];
    assert_eq!(output.name, "채팅 편집본.mp4");
    assert_eq!(output.source_job_id, Some(job_id));
    assert!((1000..=1220).contains(&output.duration_ms.unwrap()));
    assert!(output.path_in(&store.media_dir()).is_file());

    let first = &reopened.scenes[0];
    let mut settings = first.settings.clone();
    let clip = settings.visual.as_mut().unwrap();
    clip.source.in_ms = 350;
    clip.trim.as_mut().unwrap().out_ms = 850;
    let cut = store
        .edit(Edit::UpdateScene {
            id: first.id,
            expected: first.revision,
            settings,
            title_card: None,
        })
        .unwrap()
        .project;
    let scene = &cut.scenes[0];
    assert_eq!(scene.settings.duration_ms, 500);
    assert_eq!(scene.settings.visual.unwrap().trim.unwrap().offset_ms, 0);
    assert_eq!(cut.scenes[1].settings.duration_ms, 700);
    let cut_file = temp.path().join("간격 없는 편집본.mp4");
    render::export(&ffmpeg, &store.media_dir(), &cut, &cut_file).unwrap();
    for time in ["0.0", "0.46"] {
        let frame = raw_frame(&ffmpeg, &cut_file, time);
        assert!(frame[center + 2] > 190 && frame[center] < 60);
    }
    let next = raw_frame(&ffmpeg, &cut_file, "0.54");
    assert!(next[center + 1] > next[center + 2]);

    store.edit(Edit::RemoveSource { id: ids[0] }).unwrap();
    let restored = store.load().unwrap();
    assert!(restored.asset(ids[0]).unwrap().removed_from_bin);
    assert_eq!(
        restored.scenes[0].settings.visual.unwrap().source.asset_id,
        ids[0]
    );
    assert!(store.load().unwrap().asset(ids[0]).unwrap().removed_from_bin);
    let red = store.clip_thumbnail(ids[0], 100).unwrap();
    let blue = store.clip_thumbnail(ids[0], 500).unwrap();
    assert!(blue.starts_with("data:image/jpeg;base64,"));
    assert_ne!(red, blue);
    assert!(
        store
            .save_clip(scene.id, scene.revision - 1, None, None)
            .is_err()
    );
    let saved = store
        .save_clip(scene.id, scene.revision, None, None)
        .unwrap();
    let subclip = saved.view.project.asset(saved.asset_id).unwrap();
    assert!(!subclip.removed_from_bin && subclip.has_audio);
    assert!((490..=540).contains(&subclip.duration_ms.unwrap()));
    assert_ne!(subclip.id, ids[0]);
    let pixels = files::media_command(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(subclip.path_in(&store.media_dir()))
        .args([
            "-vf",
            "scale=1:1",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(pixels.status.success() && !pixels.stdout.is_empty());
    assert!(
        pixels
            .stdout
            .as_chunks::<3>()
            .0
            .iter()
            .all(|rgb| rgb[2] > 190 && rgb[0] < 60)
    );

    let mut settings = scene.settings.clone();
    let clip = settings.visual.as_mut().unwrap();
    clip.source.in_ms = 0;
    clip.trim.as_mut().unwrap().out_ms = 1200;
    store
        .edit(Edit::UpdateScene {
            id: scene.id,
            expected: scene.revision,
            settings,
            title_card: None,
        })
        .unwrap();
    let expanded = store.load().unwrap();
    assert_eq!(expanded.scenes[0].settings.duration_ms, 1200);
    assert_eq!(
        expanded.scenes[0].settings.visual.unwrap().source.asset_id,
        ids[0]
    );
    check_multi_clip_export(&store, &ffmpeg, &expanded, &ids, temp.path());
    check_generated_chat_sources(&store, &ffmpeg, ids[1]);
}

fn check_generated_chat_sources(store: &ProjectStore, ffmpeg: &Path, photo: uuid::Uuid) {
    let render = |ids: Vec<uuid::Uuid>, clip: serde_json::Value| {
        let recipe = serde_json::json!({
            "jobId": uuid::Uuid::new_v4(), "assetIds": ids, "name": "생성 테스트", "clips": [clip],
        });
        store.render_source(serde_json::from_value(recipe).unwrap())
    };
    let mut clip = serde_json::json!({
        "durationMs":1000,"visualAssetId":null,"visualInMs":0,"audioAssetId":null,
        "audioInMs":0,"caption":"", "photoMotion":"none",
    });
    assert!(render(vec![], clip.clone()).is_err());
    clip["caption"] = "대본으로 만든 영상".into();
    let text = render(vec![], clip.clone()).unwrap();
    let path = text
        .view
        .project
        .asset(text.asset_id)
        .unwrap()
        .path_in(&store.media_dir());
    let frame = raw_frame(ffmpeg, &path, "0.5");
    assert!(
        frame[1280 * 330 * 3..1280 * 390 * 3]
            .chunks_exact(3)
            .filter(|rgb| rgb.iter().all(|c| *c > 200))
            .count()
            > 100
    );

    clip["caption"] = "".into();
    clip["visualAssetId"] = serde_json::json!(photo);
    for motion in ["zoomIn", "zoomOut"] {
        clip["photoMotion"] = motion.into();
        let source = render(vec![photo], clip.clone()).unwrap();
        let path = source
            .view
            .project
            .asset(source.asset_id)
            .unwrap()
            .path_in(&store.media_dir());
        let red_left = |time| {
            let frame = raw_frame(ffmpeg, &path, time);
            frame[1280 * 225 * 3..1280 * 226 * 3]
                .chunks_exact(3)
                .position(|rgb| rgb[0] > 180 && rgb[1] < 80)
                .unwrap()
        };
        let start = red_left("0.0");
        let end = red_left("0.9");
        assert!(if motion == "zoomIn" {
            end + 40 < start
        } else {
            start + 40 < end
        });
    }

    clip["photoMotion"] = "none".into();
    clip["layout"] = "slideToLeft".into();
    clip["sideText"] = "스킬 설명\n두 번째 줄".into();
    clip["captionChanges"] = serde_json::json!([{"startMs":600,"text":"바뀐 자막"}]);
    let path_for = |result: super::RenderedSource| {
        result
            .view
            .project
            .asset(result.asset_id)
            .unwrap()
            .path_in(&store.media_dir())
    };
    let output = path_for(render(vec![photo], clip.clone()).unwrap());
    let first = raw_frame(ffmpeg, &output, "0.1");
    let last = raw_frame(ffmpeg, &output, "0.9");
    let green = |frame: &[u8], x: usize, y: usize| {
        let rgb = &frame[(1280 * y + x) * 3..][..3];
        rgb[1] > 90 && rgb[0] < 60 && rgb[2] < 60
    };
    assert!(!green(&first, 100, 340) && green(&first, 850, 340));
    assert!(green(&last, 100, 340) && !green(&last, 850, 340));
    let light_pixels = |frame: &[u8], xs: std::ops::Range<usize>, ys: std::ops::Range<usize>| {
        ys.map(|y| {
            xs.clone()
                .filter(|x| {
                    let rgb = &frame[(1280 * y + x) * 3..][..3];
                    rgb[0] > 205 && rgb[1] > 180
                })
                .count()
        })
        .sum::<usize>()
    };
    assert!(light_pixels(&first, 664..1230, 240..420) < 5);
    assert!(light_pixels(&last, 664..1230, 240..420) > 60);
    assert!(light_pixels(&first, 200..1000, 580..710) < 5);
    assert!(light_pixels(&last, 200..1000, 580..710) > 50);

    clip["layout"] = "full".into();
    clip["sideText"] = "".into();
    clip["visualKeyframes"] = serde_json::json!([
        {"timeMs":0,"centerX":50,"centerY":50,"scalePercent":100},
        {"timeMs":200,"centerX":50,"centerY":50,"scalePercent":100},
        {"timeMs":800,"centerX":25,"centerY":25,"scalePercent":50}
    ]);
    let output = path_for(render(vec![photo], clip.clone()).unwrap());
    let first = raw_frame(ffmpeg, &output, "0.1");
    let middle = raw_frame(ffmpeg, &output, "0.5");
    let last = raw_frame(ffmpeg, &output, "0.9");
    assert!(green(&first, 1150, 490));
    assert!(green(&middle, 850, 450) && !green(&middle, 1150, 490));
    assert!(green(&last, 600, 300) && !green(&last, 680, 300) && !green(&last, 600, 390));
    clip["visualKeyframes"][1]["timeMs"] = 0.into();
    assert!(render(vec![photo], clip).is_err());
}

fn check_multi_clip_export(
    store: &ProjectStore,
    ffmpeg: &Path,
    project: &super::model::Project,
    ids: &[uuid::Uuid],
    work: &Path,
) {
    let scene = &project.scenes[0];
    let first = uuid::Uuid::new_v4();
    let still = uuid::Uuid::new_v4();
    let repeated = uuid::Uuid::new_v4();
    let request = serde_json::json!({
        "type":"updateScene","id":scene.id,"expected":scene.revision,
        "settings":{"text":"혼합 구간","caption":"공통 자막","durationMs":1,"visual":null,
            "audio":{"assetId":ids[2],"inMs":2100},
            "clips":[
                {"id":first,"assetId":ids[0],"inMs":350,"durationMs":400,"trim":{"offsetMs":0,"outMs":750}},
                {"id":still,"assetId":ids[1],"inMs":0,"durationMs":400},
                {"id":repeated,"assetId":ids[0],"inMs":950,"durationMs":200,"trim":{"offsetMs":0,"outMs":1150}}
            ]
        }
    });
    let edited = store
        .edit(serde_json::from_value(request).unwrap())
        .unwrap();
    assert_eq!(edited.project.scenes[0].settings.duration_ms, 1000);
    store
        .edit(Edit::RenameSource {
            id: ids[1],
            name: "상품 이미지".into(),
        })
        .unwrap();
    let saved = store.load().unwrap();
    assert_eq!(saved.asset(ids[1]).unwrap().name, "상품 이미지");
    assert_eq!(
        saved.scenes[0].settings.clips[1].visual.source.asset_id,
        ids[1]
    );
    assert!(
        saved
            .asset(ids[1])
            .unwrap()
            .path_in(&store.media_dir())
            .is_file()
    );
    let destination = work.join("클립 세 개.mp4");
    render::export(ffmpeg, &store.media_dir(), &saved, &destination).unwrap();
    for (time, channel) in [("0.15", 2), ("0.60", 1), ("0.93", 0)] {
        let frame = raw_frame(ffmpeg, &destination, time);
        let rgb = &frame[(1280 * 200 + 640) * 3..][..3];
        assert!(
            rgb[channel] > 90 && rgb[channel] > rgb[(channel + 1) % 3],
            "wrong clip at {time}: {rgb:?}"
        );
    }
    // Overlays cross two base cuts and continue after the last base clip.
    let mut layered = store.load().unwrap();
    let mut settings = layered.scenes[0].settings.clone();
    settings.overlays = serde_json::from_value(serde_json::json!([
        {"id":uuid::Uuid::new_v4(),"assetId":ids[1],"startMs":200,"durationMs":1000,"inMs":0,"size":50,"x":50,"y":50},
        {"id":uuid::Uuid::new_v4(),"assetId":ids[0],"startMs":400,"durationMs":300,"inMs":900,"size":25,"x":50,"y":50}
    ])).unwrap();
    layered
        .edit_scene(scene.id, layered.scenes[0].revision, settings)
        .unwrap();
    assert_eq!(layered.scenes[0].settings.duration_ms, 1200);
    let dest = work.join("겹침.mp4");
    render::export(ffmpeg, &store.media_dir(), &layered, &dest).unwrap();
    for (time, x, channel) in [
        ("0.12", 640, 2),
        ("0.27", 640, 1),
        ("0.27", 190, 2),
        ("0.55", 640, 0),
        ("0.55", 440, 1),
        ("0.82", 640, 1),
        ("1.10", 640, 1),
    ] {
        let frame = raw_frame(ffmpeg, &dest, time);
        let rgb = &frame[(1280 * 360 + x) * 3..][..3];
        assert!(
            rgb[channel] > 85 && rgb[channel] > rgb[(channel + 1) % 3],
            "layer frame {time} at {x}: {rgb:?}"
        );
    }
    // Two clips on one upper track, a gap on the lower track, and audio from the upper video.
    let mut settings = saved.scenes[0].settings.clone();
    settings.caption.clear();
    settings.audio = None;
    settings.clips[0].gap_ms = 200;
    settings.clips[0].visual.muted = true;
    settings.clips[0].visual.frame = Some(super::model::ClipFrame {
        size: 50,
        x: 0,
        y: 0,
    });
    let upper_id = uuid::Uuid::new_v4();
    settings.overlays = serde_json::from_value(serde_json::json!([
        {"id":uuid::Uuid::new_v4(),"assetId":ids[1],"track":1,"startMs":650,"durationMs":300,"inMs":0,"size":100,"x":50,"y":50},
        {"id":upper_id,"assetId":ids[0],"track":1,"sound":true,"startMs":0,"durationMs":200,"inMs":900,"size":100,"x":50,"y":50}
    ])).unwrap();
    layered
        .edit_scene(scene.id, layered.scenes[0].revision, settings)
        .unwrap();
    assert_eq!(layered.scenes[0].settings.overlays[0].id, upper_id);
    let dest = work.join("공통 트랙.mp4");
    render::export(ffmpeg, &store.media_dir(), &layered, &dest).unwrap();
    let first_frame = raw_frame(ffmpeg, &dest, "0.10");
    let center = &first_frame[(1280 * 360 + 640) * 3..][..3];
    assert!(center[0] > 90 && center[0] > center[2]);
    let bottom = raw_frame(ffmpeg, &dest, "0.30");
    let inside = &bottom[(1280 * 180 + 320) * 3..][..3];
    let outside = &bottom[(1280 * 500 + 940) * 3..][..3];
    assert!(inside[2] > 90 && outside.iter().all(|&v| v < 55));
    let pcm = files::media_command(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&dest)
        .args(["-t", "0.18", "-vn", "-ac", "1", "-f", "s16le", "-"])
        .output()
        .unwrap();
    assert!(
        pcm.status.success()
            && pcm
                .stdout
                .chunks_exact(2)
                .any(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs() > 100)
    );
    let clip = store
        .save_clip(scene.id, saved.scenes[0].revision, Some(repeated), None)
        .unwrap();
    assert!(
        (180..=250).contains(
            &clip
                .view
                .project
                .asset(clip.asset_id)
                .unwrap()
                .duration_ms
                .unwrap()
        )
    );
}

fn raw_frame(ffmpeg: &Path, destination: &Path, time: &str) -> Vec<u8> {
    let frame = files::media_command(ffmpeg)
        .args(["-v", "error", "-ss", time, "-i"])
        .arg(destination)
        .args(["-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
        .output()
        .unwrap();
    assert!(frame.status.success());
    assert_eq!(frame.stdout.len(), 1280 * 720 * 3);
    frame.stdout
}

#[test]
fn chat_rename_preserves_scene_data_and_can_restore_numbered_name() {
    use serde_json::json;
    use uuid::Uuid;

    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let original = store
        .mutate(|p, _, _| {
            p.add_scene("첫 대본".into());
            p.add_scene("둘째 대본".into());
            p.scenes[1].settings.notes = "기존 메모".into();
            Ok(())
        })
        .unwrap()
        .project;
    let target = original.scenes[1].id;
    let rename = |base: &Project, job: Uuid, name: &str| {
        let versions: Vec<_> = base
            .scenes
            .iter()
            .map(|s| json!({"id":s.id,"revision":s.revision}))
            .collect();
        store.edit(
            serde_json::from_value(
                json!({"type":"applyChatTimeline","jobId":job,"base":versions,
            "operations":[{"type":"renameScene","sceneId":target,"name":name}]}),
            )
            .unwrap(),
        )
    };
    let job = Uuid::new_v4();
    let named = rename(&original, job, " 오프닝 ").unwrap().project;
    let mut expected = original.scenes[1].settings.clone();
    expected.name = "오프닝".into();
    assert!(named.scenes[1].settings == expected);
    assert!(named.scenes[0].settings == original.scenes[0].settings);
    assert_eq!(
        rename(&original, job, " 오프닝 ").unwrap().project.scenes[1].revision,
        named.scenes[1].revision
    );
    assert!(rename(&original, Uuid::new_v4(), "이전 요청").is_err());
    assert_eq!(
        store.load().unwrap().scenes[1].settings.name,
        "오프닝"
    );
    let manual = |rev, name: &str| {
        store.edit(
            serde_json::from_value(json!({
                "type":"renameScene", "id":target, "expected":rev, "name":name
            }))
            .unwrap(),
        )
    };
    assert!(manual(original.scenes[1].revision, "이전 이름").is_err());
    let reset = manual(named.scenes[1].revision, "").unwrap();
    assert!(reset.project.scenes[1].settings == original.scenes[1].settings);
    let token = serde_json::from_value(json!(reset.undo.unwrap())["token"].clone()).unwrap();
    assert!(store.edit(Edit::Undo { token }).unwrap().project.scenes[1].settings == expected);
}

#[test]
fn chat_scene_notes_append_replace_and_undo_without_changing_other_scene_data() {
    check_chat_writing_edits("editSceneNotes", "notes", "기존 메모");
}

#[test]
fn chat_scene_text_append_replace_and_undo_without_changing_other_scene_data() {
    check_chat_writing_edits("editSceneText", "text", "둘째 구간");
}

fn check_chat_writing_edits(operation: &str, field: &str, initial: &str) {
    use serde_json::json;
    use uuid::Uuid;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let original = store
        .mutate(|p, _, _| {
            p.add_scene("첫 구간".into());
            p.add_scene("둘째 구간".into());
            p.scenes[1].settings.notes = "기존 메모".into();
            Ok(())
        })
        .unwrap()
        .project;
    let id = original.scenes[1].id;
    let mut base: Vec<_> = original
        .scenes
        .iter()
        .map(|s| json!({"id":s.id,"revision":s.revision}))
        .collect();
    let edit = |job, base: &[Value], mode, text| {
        store.edit(
            serde_json::from_value(json!({
                "type":"applyChatTimeline","jobId":job,"base":base,
                "operations":[{"type":operation,"sceneId":id,"mode":mode,"text":text}]
            }))
            .unwrap(),
        )
    };
    let first = Uuid::new_v4();
    let appended = edit(first, &base, "append", "자막은 오른쪽").unwrap();
    let mut expected = json!(original.scenes[1].settings);
    expected[field] = json!(format!("{initial}\n자막은 오른쪽"));
    assert_eq!(json!(appended.project.scenes[1].settings), expected);
    assert!(appended.project.scenes[0].settings == original.scenes[0].settings);
    let replayed = edit(first, &base, "append", "자막은 오른쪽").unwrap();
    assert_eq!(json!(replayed.project.scenes[1].settings), expected);
    assert!(edit(Uuid::new_v4(), &base, "replace", "이전 요청").is_err());
    base[1]["revision"] = appended.project.scenes[1].revision.into();
    let replaced = edit(Uuid::new_v4(), &base, "replace", "기존 메모\n자막은 왼쪽").unwrap();
    assert_eq!(
        json!(store.load().unwrap().scenes[1])[field],
        "기존 메모\n자막은 왼쪽"
    );
    let token = serde_json::from_value(json!(replaced.undo.unwrap())["token"].clone()).unwrap();
    let undone = store.edit(Edit::Undo { token }).unwrap();
    assert_eq!(json!(undone.project.scenes[1].settings), expected);
}

#[test]
fn chat_timeline_commits_once_and_undo_restores_the_entire_request() {
    use serde_json::json;
    use uuid::Uuid;
    let dir = tempdir().unwrap();
    let store = ProjectStore::new(dir.path().into());
    let video = Uuid::new_v4();
    let photo = Uuid::new_v4();
    let original = store.mutate(|p,_,_| {
        p.add_scene("시작".into());
        p.add_scene("다음 장면".into());
        for (id,kind,ext,duration) in [(video,"video","mp4",Some(8000)),(photo,"image","png",None)] {
            p.assets.push(serde_json::from_value(json!({"id":id,"kind":kind,"extension":ext,"name":ext,"durationMs":duration,"hasAudio":false})).unwrap());
        }
        p.scenes[0].settings.visual=Some(VisualPlacement{frame:None,muted:false,source:Placement{asset_id:video,in_ms:500},trim:None});
        Ok(())
    }).unwrap().project;
    let a = original.scenes[0].id;
    let b = original.scenes[1].id;
    let base: Vec<_> = original
        .scenes
        .iter()
        .map(|s| json!({"id":s.id,"revision":s.revision}))
        .collect();
    let job = Uuid::new_v4();
    let apply = |job, base: &[Value], ops: Value| {
        store.edit(
            serde_json::from_value(json!({
                "type":"applyChatTimeline","jobId":job,"base":base,"operations":ops
            }))
            .unwrap(),
        )
    };
    let insert = json!({"type":"insert","sceneId":b,"assetId":photo,"position":null,"startMs":null,"endMs":1800});
    let split = json!({"type":"split","sceneId":a,"clip":1,"atMs":1000});
    let movement = json!({"type":"moveClip","sceneId":a,"clip":2,"position":1});
    let trim = json!({"type":"trim","sceneId":a,"clip":1,"startMs":2500,"endMs":4000});
    let order = json!({"type":"moveScene","sceneId":b,"position":1});
    let remove = json!({"type":"removeClip","sceneId":b,"clip":1});
    let ops = json!([insert, remove, insert, split, movement, trim, order]);
    assert!(
        apply(
            job,
            &base,
            json!([insert,{"type":"split","sceneId":b,"clip":1,"atMs":200}])
        )
        .is_err()
    );
    assert_eq!(
        serde_json::to_value(store.load().unwrap()).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    let view = apply(job, &base, ops.clone()).unwrap();
    assert_eq!(view.project.scenes[0].id, b);
    assert_eq!(
        view.project.scenes[0].settings.clips[0]
            .visual
            .source
            .asset_id,
        photo
    );
    let clips = &view.project.scenes[1].settings.clips;
    assert_eq!(
        clips
            .iter()
            .map(|c| (c.visual.source.in_ms, c.duration_ms))
            .collect::<Vec<_>>(),
        [(2500, 1500), (500, 1000)]
    );
    assert_ne!(clips[0].id, clips[1].id);
    assert!(apply(Uuid::new_v4(), &base, ops.clone()).is_err());
    let token: Uuid =
        serde_json::from_value(serde_json::to_value(view.undo.unwrap()).unwrap()["token"].clone())
            .unwrap();
    let undone = store.edit(Edit::Undo { token }).unwrap();
    assert!(undone.undo.is_none());
    let restored = apply(job, &base, ops).unwrap();
    assert_eq!(restored.project.timeline_jobs, [job]);
    for (scene, old) in restored.project.scenes.iter().zip(&original.scenes) {
        assert_eq!(scene.id, old.id);
        assert!(scene.settings == old.settings);
    }
}

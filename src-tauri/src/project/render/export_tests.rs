use super::{Export, encoder::Encoder};
use crate::project::{ass::AssTrack, model::Project};
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::tempdir;
use uuid::Uuid;

const CAPTION: &str = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1280
PlayResY: 720
[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,50,&H00FFFFFF,&H00FFFFFF,0,0,0,0,0,0,100,100,0,0,1,0,0,2,10,10,20,1
[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:00.60,Default,,0,0,0,,ORIGINAL
"#;

fn ffmpeg() -> PathBuf {
    PathBuf::from(env::var_os("CUTROOM_TEST_TOOLS").unwrap())
        .join(format!("ffmpeg{}", env::consts::EXE_SUFFIX))
}

fn run(command: &mut Command) -> Vec<u8> {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn video(ff: &Path, file: &Path, color: &str, hz: u32) {
    run(Command::new(ff)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c={color}:s=160x90:r=30:d=0.6"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={hz}:duration=0.6:sample_rate=48000"),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ])
        .arg(file));
}

fn frame(ff: &Path, file: &Path, at: &str) -> Vec<u8> {
    let bytes = run(Command::new(ff)
        .args(["-v", "error", "-ss", at, "-i"])
        .arg(file)
        .args([
            "-frames:v",
            "1",
            "-an",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "pipe:1",
        ]));
    assert_eq!(bytes.len(), 1280 * 720 * 3);
    bytes
}

fn sound(ff: &Path, file: &Path) -> Vec<i16> {
    run(Command::new(ff)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args([
            "-map", "0:a", "-ar", "8000", "-ac", "1", "-f", "s16le", "pipe:1",
        ]))
    .chunks_exact(2)
    .map(|b| i16::from_le_bytes([b[0], b[1]]))
    .collect()
}

#[test]
#[ignore = "Requires CUTROOM_TEST_TOOLS with FFmpeg"]
fn repeat_export_reuses_only_current_parts_and_remixes_bgm() {
    let ff = ffmpeg();
    let dir = tempdir().unwrap();
    let media = dir.path().join("media");
    let cache = dir.path().join("cache");
    fs::create_dir(&media).unwrap();
    let mut project = Project::default();
    for (color, hz) in [("red", 300), ("blue", 600)] {
        let id = Uuid::new_v4();
        let asset = serde_json::from_value(json!({"id":id,"name":color,"kind":"video","extension":"mp4","durationMs":600,"hasAudio":true})).unwrap();
        project.assets.push(asset);
        video(&ff, &project.asset(id).unwrap().path_in(&media), color, hz);
        project.add_scene(color.into());
        let settings = &mut project.scenes.last_mut().unwrap().settings;
        settings.duration_ms = 600;
        settings.visual = Some(serde_json::from_value(json!({"assetId":id,"inMs":0})).unwrap());
    }
    project.scenes[0].settings.caption_ass = Some(AssTrack {
        name: "test.ass".into(),
        source: CAPTION.replace("ORIGINAL", "I"),
    });
    let export = |project: &Project, filename: &str, expected| {
        let out = dir.path().join(filename);
        let report = Export {
            ffmpeg: &ff,
            media: &media,
            project,
            styles: &[],
            cache_root: Some(&cache),
        }
        .write(&out, 0..2)
        .unwrap();
        assert_eq!((report.rendered, report.reused), expected, "{filename}");
        out
    };

    let first = export(&project, "first.mp4", (2, 0));
    let warm = export(&project, "repeat.mp4", (0, 2));
    let top_frame = frame(&ff, &first, "0.2");
    let next_frame = frame(&ff, &first, "0.8");
    assert!(top_frame[0] > 200 && top_frame[2] < 20);
    assert!(next_frame[2] > 200 && next_frame[0] < 20);
    assert!(frame(&ff, &warm, "0.2") == top_frame);
    assert!(frame(&ff, &warm, "0.8") == next_frame);
    let original_audio = sound(&ff, &first);
    assert!(sound(&ff, &warm) == original_audio);
    for (offset, hz) in [(1600, 300), (6400, 600)] {
        let cycles = original_audio[offset..offset + 800]
            .windows(2)
            .filter(|p| p[0] < 0 && p[1] >= 0)
            .count();
        assert!(
            cycles.abs_diff(hz / 10) <= 2,
            "unexpected tone: {cycles} cycles in 100 ms"
        );
    }

    project.scenes[0]
        .settings
        .caption_ass
        .as_mut()
        .unwrap()
        .source = CAPTION.replace("ORIGINAL", "MMMMMMMMMM");
    let caption_edit = export(&project, "new-caption.mp4", (1, 1));
    let changed_frame = frame(&ff, &caption_edit, "0.2");
    let white = |rgb: &[u8]| {
        rgb.chunks_exact(3)
            .filter(|p| p.iter().all(|&v| v > 180))
            .count()
    };
    assert!(white(&changed_frame) > white(&top_frame) + 100);
    assert!(frame(&ff, &caption_edit, "0.8") == next_frame);

    // Replacing a file in the media folder doesn't change any scene revision.
    let second_file = project.assets[1].path_in(&media);
    video(&ff, &second_file, "lime", 600);
    let source_edit = export(&project, "new-source.mp4", (1, 1));
    let changed_source = frame(&ff, &source_edit, "0.8");
    assert!(changed_source[1] > 200 && changed_source[2] < 20);
    assert!(frame(&ff, &source_edit, "0.2") == changed_frame);

    let song = Uuid::new_v4();
    project.assets.push(serde_json::from_value(json!({"id":song,"name":"song","kind":"audio","extension":"wav","durationMs":1200,"hasAudio":true})).unwrap());
    run(Command::new(&ff)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:duration=1.2:sample_rate=48000",
        ])
        .arg(project.asset(song).unwrap().path_in(&media)));
    project.bgm.push(serde_json::from_value(json!({"id":Uuid::new_v4(),"assetId":song,"volume":100,"fadeInMs":0,"fadeOutMs":0,
        "start":{"sceneId":project.scenes[0].id,"offsetMs":0},"end":{"sceneId":project.scenes[1].id,"offsetMs":null}})).unwrap());
    let bgm = export(&project, "new-music.mp4", (0, 2));
    assert!(frame(&ff, &bgm, "0.2") == changed_frame && frame(&ff, &bgm, "0.8") == changed_source);
    let mixed_audio = sound(&ff, &bgm);
    let rms = |samples: &[i16]| {
        (samples[1600..2400]
            .iter()
            .map(|&v| f64::from(v).powi(2))
            .sum::<f64>()
            / 800.0)
            .sqrt()
    };
    assert!(rms(&mixed_audio) > 1.2 * rms(&original_audio));

    project.bgm.clear();
    project.scenes.swap(0, 1);
    let reordered = export(&project, "reordered.mp4", (0, 2));
    assert!(frame(&ff, &reordered, "0.2") == changed_source);
    assert!(frame(&ff, &reordered, "0.8") == changed_frame);
}

#[test]
#[ignore = "Requires CUTROOM_TEST_TOOLS with FFmpeg"]
fn auto_and_requested_hardware_export_decodable_mp4() {
    let ff = ffmpeg();
    let dir = tempdir().unwrap();
    let mut project = Project::default();
    project.add_scene("short scene".into());
    project.scenes[0].settings.duration_ms = 300;
    let export = Export {
        ffmpeg: &ff,
        media: dir.path(),
        project: &project,
        styles: &[],
        cache_root: None,
    };
    let auto = dir.path().join("auto.mp4");
    let report = export.write(&auto, 0..1).unwrap();
    eprintln!("Selected encoder: {:?}", report.encoder);
    assert_eq!((report.rendered, report.reused), (1, 0));
    frame(&ff, &auto, "0.1");

    // On hosts without a working Intel encoder this exercises the real FFmpeg failure and CPU retry.
    let other = dir.path().join("requested-qsv.mp4");
    let report = export
        .write_with_encoder(&other, 0..1, Encoder::Qsv)
        .unwrap();
    eprintln!("Requested QSV, finished with {:?}", report.encoder);
    assert!(matches!(report.encoder, Encoder::Qsv | Encoder::Cpu));
    frame(&ff, &other, "0.1");
}

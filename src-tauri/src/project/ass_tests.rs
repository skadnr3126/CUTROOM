use super::{
    ProjectStore,
    ass::{AssTrack, ass_rows, ass_update},
    model::Project,
    render,
};
use serde_json::json;

pub(super) const SOURCE: &str = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1280
PlayResY: 720
[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Caption,Malgun Gothic,52,&H00FFFFFF,&H00FFFFFF,0,0,-1,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1
Style: Panel,Arial,20,&H000000FF,0,0,0,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1
[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.50,0:00:01.70,Panel,,0,0,0,,{\an7\pos(100,500)\p1}m 0 0 l 1000 0 1000 150 0 150
Dialogue: 1,0:00:00.50,0:00:01.70,Caption,,0,0,0,,{\pos(600,550)\fad(100,150)}처음,\N둘째 줄
"#;

#[test]
fn deleting_timeline_caption_is_undoable_and_empty_ass_reuses_its_design() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(temp.path().into());
    let view = store
        .mutate(|p, _, _| {
            p.add_scene("삭제".into());
            Ok(())
        })
        .unwrap();
    let id = view.project.scenes[0].id;
    let edit = |expected, tool| {
        store.edit(
            serde_json::from_value(json!({
                "type":"sceneTool","sceneId":id,"expected":expected,"tool":tool
            }))
            .unwrap(),
        )
    };
    let delete = |end| json!({"type":"deleteCaption","startMs":500,"endMs":end});
    edit(
        0,
        json!({"type":"setCaptionCues","cues":[{"startMs":500,"endMs":1700,"text":"일반"}]}),
    )
    .unwrap();
    assert!(edit(1, delete(1800)).is_err());
    assert!(
        edit(1, delete(1700)).unwrap().project.scenes[0]
            .settings
            .caption_cues
            .is_empty()
    );
    let original = format!("{SOURCE}Dialogue: 1,0:00:00.50,0:00:01.70,Caption,,0,0,0,,다른 줄\n");
    edit(
        2,
        json!({"type":"setAssTrack","name":"자막.ass","source":original}),
    )
    .unwrap();
    assert!(edit(2, delete(1700)).is_err());
    let deleted = edit(3, delete(1700)).unwrap();
    let source = &deleted.project.scenes[0]
        .settings
        .caption_ass
        .as_ref()
        .unwrap()
        .source;
    assert!(!source.contains("다른 줄") && !source.contains("처음"));
    assert!(ass_rows(source.clone()).unwrap().is_empty());
    assert!(super::ass::ass_ranges(source.clone()).unwrap().is_empty());
    let replaced = super::ass::insert(source, 0, 4000, "다시 추가\n둘째 줄").unwrap();
    let rows = ass_rows(replaced.clone()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "다시 추가\n둘째 줄");
    assert!(replaced.contains(r"{\pos(600,550)\fad(100,150)}다시 추가\N둘째 줄"));
    assert!(replaced.contains("0:00:00.00,0:00:02.00,Panel"));
    let restored = store
        .edit(
            serde_json::from_value(
                json!({"type":"undo","token":json!(deleted.undo.unwrap())["token"]}),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        restored.project.scenes[0]
            .settings
            .caption_ass
            .as_ref()
            .unwrap()
            .source,
        original
    );
}

#[test]
fn add_ass_rows_inherits_design_and_time_and_survives_independent_edits() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(temp.path().into());
    let view = store
        .mutate(|p, _, _| {
            p.add_scene("줄 추가".into());
            Ok(())
        })
        .unwrap();
    let id = view.project.scenes[0].id;
    let edit = |expected, tool| {
        store.edit(
            serde_json::from_value(json!({
                "type":"sceneTool","sceneId":id,"expected":expected,"tool":tool
            }))
            .unwrap(),
        )
    };
    let source = SOURCE.to_owned()
        + "Dialogue: 1,0:00:00.50,0:00:01.70,Caption,,0,0,0,,{\\pos(600,620)}나머지\n";
    edit(
        0,
        json!({"type":"setAssTrack","name":"자막.ass","source":source}),
    )
    .unwrap();
    let mut rows = ass_rows(source.clone()).unwrap();
    let template = rows[0].line;
    rows[0].start_ms = 600;
    rows[1].text = "수정한 다른 줄".into();
    let added = edit(
        1,
        json!({"type":"editAssRows","edits":rows,"additions":[
        {"templateLine":template,"text":"추가 A"},{"templateLine":template,"text":"추가 B"}]}),
    )
    .unwrap();
    let saved = added.project.scenes[0]
        .settings
        .caption_ass
        .as_ref()
        .unwrap();
    let records = ass_rows(saved.source.clone()).unwrap();
    assert_eq!(
        records.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
        ["처음,\n둘째 줄", "추가 A", "추가 B", "수정한 다른 줄"]
    );
    assert_eq!((records[1].start_ms, records[1].end_ms), (600, 1700));
    let first = saved.source.lines().nth(records[1].line).unwrap();
    let second = saved.source.lines().nth(records[2].line).unwrap();
    assert!(first.contains(r"\fad(100,150)") && !first.contains(r"\pos(600,550)"));
    assert_ne!(first.split('}').next(), second.split('}').next());
    let mut removed = records[1].clone();
    removed.text.clear();
    let updated = ass_update(saved.source.clone(), vec![removed], 4000, None).unwrap();
    assert!(
        !updated.contains("추가 A")
            && updated.contains("추가 B")
            && updated.contains("수정한 다른 줄")
    );
    let restored = store
        .edit(
            serde_json::from_value(
                json!({"type":"undo","token":json!(added.undo.unwrap())["token"]}),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        restored.project.scenes[0]
            .settings
            .caption_ass
            .as_ref()
            .unwrap()
            .source,
        source
    );
}

fn add_rows_to_template(source: &str, index: usize, texts: &[&str]) -> String {
    let template_line = ass_rows(source.into()).unwrap()[index].line;
    let additions = texts
        .iter()
        .map(|text| super::ass::NewRow {
            template_line,
            text: (*text).into(),
        })
        .collect();
    ass_update(source.into(), vec![], 120_000, Some(additions)).unwrap()
}

#[test]
fn add_ass_row_reuses_the_neighboring_second_line_position() {
    let header = SOURCE
        .split_once("Dialogue:")
        .unwrap()
        .0
        .replace("1280", "1920")
        .replace("720", "1080")
        .replace("Malgun Gothic,52", "Noto Sans KR,71.5");
    let events = r#"Dialogue: 1,0:01:26.30,0:01:30.80,Caption,,0,0,0,,{\fscx102\fscy102\pos(960,956)}앞 자막 첫 줄
Dialogue: 1,0:01:26.30,0:01:30.80,Caption,,0,0,0,,{\fscx102\fscy102\pos(960,1014)}앞 자막 둘째 줄
Dialogue: 0,0:01:30.80,0:01:35.80,Panel,,0,0,0,,{\an7\pos(120,920)\p1}m 0 0 l 1680 0 1680 135 0 135
Dialogue: 1,0:01:30.80,0:01:35.80,Caption,,0,0,0,,{\fscx102\fscy102\pos(960,956)}극딜 안정성 또한 떨어집니다.\N
"#;
    let source = header + events;
    let saved = add_rows_to_template(&source, 2, &["추가한 둘째 줄"]);
    let expected = r"Dialogue: 1,0:01:30.80,0:01:35.80,Caption,,0,0,0,,{\fscx102\fscy102\pos(960,1014)}추가한 둘째 줄";
    assert_eq!(saved, format!("{source}{expected}\n"));
}

#[test]
fn add_ass_row_ignores_trailing_line_breaks_when_estimating_space() {
    let add_below = |body: &str| {
        let source = SOURCE.replace(r"처음,\N둘째 줄", body);
        let result = add_rows_to_template(&source, 0, &["새 줄"]);
        result.lines().last().unwrap().to_owned()
    };
    assert_eq!(add_below("한 줄"), add_below(r"한 줄\N"));
    assert_ne!(add_below("한 줄"), add_below(r"내용\N둘째 줄"));
}

#[test]
fn add_ass_rows_near_the_bottom_keeps_the_stack_in_reading_order_on_screen() {
    let source = SOURCE
        .replace(r"처음,\N둘째 줄", "원래 줄")
        .replace(r"\pos(600,550)", r"\pos(600,700)");
    let result = add_rows_to_template(&source, 0, &["추가 A", "추가 B"]);
    let rows = ass_rows(result.clone()).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
        ["원래 줄", "추가 A", "추가 B"]
    );
    let y: Vec<f64> = rows
        .iter()
        .map(|row| {
            let line = result.lines().nth(row.line).unwrap();
            let (_, coords) = line.split_once(r"\pos(600,").unwrap();
            coords.split_once(')').unwrap().0.parse().unwrap()
        })
        .collect();
    assert!(y[0] < y[1] && y[1] < y[2], "{y:?}");
    // With center alignment even the outer rows need half a font's height inside the frame.
    assert!(y[0] >= 26.0 && y[2] <= 720.0 - 26.0, "{y:?}");
}

#[test]
fn add_ass_row_ignores_other_columns_and_unrelated_single_row_cues() {
    let source = SOURCE
        .replace(r"처음,\N둘째 줄", "기준")
        .replace(r"\fad(100,150)", "");
    let unrelated = r"Dialogue: 1,0:00:02.00,0:00:03.00,Caption,,0,0,0,,{\pos(200,550)}다른 열 첫 줄
Dialogue: 1,0:00:02.00,0:00:03.00,Caption,,0,0,0,,{\pos(200,580)}다른 열 둘째 줄
Dialogue: 1,0:00:03.00,0:00:04.00,Caption,,0,0,0,,{\pos(600,550)}따로 쓰인 줄
Dialogue: 1,0:00:04.00,0:00:05.00,Caption,,0,0,0,,{\pos(600,580)}다른 시각의 줄
";
    let baseline = add_rows_to_template(&source, 0, &["추가"]);
    let saved = add_rows_to_template(&(source + unrelated), 0, &["추가"]);
    assert_eq!(saved, baseline + unrelated);
}

#[test]
fn adding_between_reopened_rows_matches_the_editor_order_on_screen() {
    let source = SOURCE
        .replace(r"처음,\N둘째 줄", "첫 줄")
        .replace("600,550", "600,350");
    let pair = add_rows_to_template(&source, 0, &["마지막 줄"]);
    let three = add_rows_to_template(&pair, 0, &["가운데 줄"]);
    let position = |source: &str, text: &str| -> f64 {
        let line = source.lines().find(|l| l.ends_with(text)).unwrap();
        line.split_once(r"\pos(600,")
            .unwrap()
            .1
            .split_once(')')
            .unwrap()
            .0
            .parse()
            .unwrap()
    };
    assert_eq!(position(&three, "가운데 줄"), position(&pair, "마지막 줄"));
    assert!(position(&three, "마지막 줄") > position(&three, "가운데 줄"));
}

#[test]
fn clearing_one_ass_row_preserves_its_sibling_and_shared_panel() {
    let sibling = "Dialogue: 2,0:00:00.50,0:00:01.70,Caption,,0,0,0,,{\\pos(600,620)}아래 줄";
    let later = "Dialogue: 1,0:00:02.00,0:00:03.00,Caption,,0,0,0,,다음 자막";
    let source = format!("{SOURCE}{sibling}\n{later}\n").replace('\n', "\r\n");
    let original = ass_rows(source.clone()).unwrap();
    for index in [0, 1] {
        let mut erase = original[index].clone();
        erase.text.clear();
        let updated = ass_update(source.clone(), vec![erase], 4000, None).unwrap();
        let remaining = ass_rows(updated.clone()).unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].text, original[1 - index].text);
        assert_eq!((remaining[0].start_ms, remaining[0].end_ms), (500, 1700));
        let kept_line = source.lines().nth(original[1 - index].line).unwrap();
        assert!(updated.contains(kept_line) && updated.contains(later));
        assert!(updated.contains("0:00:00.50,0:00:01.70,Panel"));
        assert!(updated.ends_with("\r\n"));
    }
}

#[test]
fn deleting_middle_caption_opens_gap_in_shared_panel() {
    let source = SOURCE
        .replace("0:00:01.70,Panel", "0:00:04.00,Panel")
        .to_owned()
        + "Dialogue: 1,0:00:01.70,0:00:03.00,Caption,,0,0,0,,중간\n"
        + "Dialogue: 1,0:00:03.00,0:00:04.00,Caption,,0,0,0,,끝\n"
        + "Dialogue: 0,0:00:01.70,0:00:03.00,Diagram,,0,0,0,,{\\p1}m 0 0 l 20 0 20 20\n";
    let updated = super::ass::delete(&source, 1700, 3000).unwrap();
    assert_eq!(ass_rows(updated.clone()).unwrap().len(), 2);
    assert!(updated.contains("0:00:00.50,0:00:01.70,Panel"));
    assert!(updated.contains("0:00:03.00,0:00:04.00,Panel"));
    assert!(updated.contains("0:00:01.70,0:00:03.00,Diagram"));
    assert!(!updated.contains("중간"));
}

#[test]
fn add_caption_from_playhead_keeps_style_timing_and_undo() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(temp.path().into());
    let view = store
        .mutate(|p, _, _| {
            p.add_scene("입력".into());
            Ok(())
        })
        .unwrap();
    let id = view.project.scenes[0].id;
    let edit = |expected, tool| {
        store.edit(
            serde_json::from_value(json!({
                "type":"sceneTool","sceneId":id,"expected":expected,"tool":tool
            }))
            .unwrap(),
        )
    };
    let add = |ms, text| json!({"type":"addCaption","atMs":ms,"text":text});
    edit(0, json!({"type":"setCaption","text":"원래 문구"})).unwrap();
    edit(1, add(0, "첫 자막\r\n둘째 줄")).unwrap();
    let reopened = ProjectStore::new(temp.path().into()).load().unwrap();
    let cues = &reopened.scenes[0].settings.caption_cues;
    assert_eq!(cues.len(), 2);
    assert_eq!(
        (&*cues[0].text, &*cues[1].text),
        ("첫 자막\n둘째 줄", "원래 문구")
    );
    assert_eq!(cues[0].end_ms, cues[1].start_ms);
    edit(
        2,
        json!({"type":"setAssTrack","name":"caption.ass","source":SOURCE}),
    )
    .unwrap();
    assert!(edit(2, add(1000, "중복")).is_err());
    assert!(edit(3, add(1000, "  ")).is_err());
    let saved = edit(3, add(1005, "직접 입력\r\n둘째 줄")).unwrap();
    let source = &saved.project.scenes[0]
        .settings
        .caption_ass
        .as_ref()
        .unwrap()
        .source;
    let rows = ass_rows(source.clone()).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].start_ms, rows[0].end_ms), (500, 1000));
    assert_eq!((rows[1].start_ms, rows[1].end_ms), (1000, 1700));
    assert_eq!(rows[1].text, "직접 입력\n둘째 줄");
    assert!(source.contains(r"{\pos(600,550)\fad(100,150)}직접 입력\N둘째 줄"));
    assert!(source.contains("0:00:00.50,0:00:01.70,Panel"));
    let restored = store
        .edit(
            serde_json::from_value(
                json!({"type":"undo","token":json!(saved.undo.unwrap())["token"]}),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        restored.project.scenes[0]
            .settings
            .caption_ass
            .as_ref()
            .unwrap()
            .source,
        SOURCE
    );
    let gap = super::ass::insert(SOURCE, 0, 5000, "빈 구간").unwrap();
    assert!(gap.contains("0:00:00.00,0:00:00.50,Panel"));
    let boundary = super::ass::insert(SOURCE, 500, 5000, "먼저").unwrap();
    let rows = ass_rows(boundary).unwrap();
    assert_eq!((rows[1].start_ms, rows[1].end_ms), (500, 1100));
    assert_eq!((rows[0].start_ms, rows[0].end_ms), (1100, 1700));
}

#[test]
fn ass_edit_preserves_design_roundtrip_and_undo() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(temp.path().into());
    let view = store
        .mutate(|p, _, _| {
            p.add_scene("테스트".into());
            Ok(())
        })
        .unwrap();
    let id = view.project.scenes[0].id;
    let edit = |req| store.edit(serde_json::from_value(req).unwrap());
    let scene_tool =
        |expected, tool| json!({"type":"sceneTool","sceneId":id,"expected":expected,"tool":tool});
    edit(scene_tool(
        0,
        json!({"type":"setAssTrack","name":"caption.ass","source":SOURCE}),
    ))
    .unwrap();
    let mut rows = ass_rows(SOURCE.into()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "처음,\n둘째 줄");
    rows[0].text = "편집한\r\n마지막 줄".into();
    rows[0].start_ms = 650;
    rows[0].end_ms = 1900;
    let saved = edit(scene_tool(1, json!({"type":"editAssRows","edits":rows}))).unwrap();
    let reopened = ProjectStore::new(temp.path().into()).load().unwrap();
    let changed = &reopened.scenes[0]
        .settings
        .caption_ass
        .as_ref()
        .unwrap()
        .source;
    assert!(changed.contains(r"{\pos(600,550)\fad(100,150)}편집한\N마지막 줄"));
    let edited_rows = ass_rows(changed.clone()).unwrap();
    assert_eq!(edited_rows.len(), 1);
    assert_eq!(edited_rows[0].text, "편집한\n마지막 줄");
    assert!(changed.contains("0:00:00.65,0:00:01.90,Panel"));
    assert!(edit(scene_tool(1, json!({"type":"removeAssTrack"}))).is_err());
    assert!(
        ass_update(
            SOURCE.into(),
            vec![super::ass::Row {
                line: rows[0].line,
                start_ms: 2000,
                end_ms: 1000,
                text: "x".into(),
                center: None,
            }],
            4000,
            None
        )
        .is_err()
    );
    let restored =
        edit(json!({"type":"undo","token":json!(saved.undo.unwrap())["token"]})).unwrap();
    assert_eq!(
        restored.project.scenes[0]
            .settings
            .caption_ass
            .as_ref()
            .unwrap()
            .source,
        SOURCE
    );
}

#[test]
#[ignore = "Requires CUTROOM_TEST_TOOLS with FFmpeg"]
fn added_positioned_row_renders_in_a_separate_visible_band() {
    let ff = std::path::PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap())
        .join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX));
    let template = ass_rows(SOURCE.into()).unwrap().remove(0);
    let added =
        serde_json::from_value(json!({"templateLine":template.line,"text":"추가한 자막"})).unwrap();
    let result = ass_update(SOURCE.into(), vec![], 4000, Some(vec![added])).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let bands = |source: &str| {
        let png = temp.path().join("row.png");
        std::fs::write(&png, super::ass_preview::sheet(&ff, source, 30, 1).unwrap()).unwrap();
        let raw = std::process::Command::new(&ff)
            .args(["-v", "error", "-i"])
            .arg(png)
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
            .output()
            .unwrap();
        assert_eq!(raw.stdout.len(), 1280 * 720 * 4);
        raw.stdout
            .chunks_exact(1280 * 4)
            .map(|row| row.chunks_exact(4).any(|p| p.iter().all(|c| *c > 180)))
            .collect::<Vec<_>>()
    };
    let rows = ass_rows(result.clone()).unwrap();
    let isolated = |i: usize| {
        let mut remove = rows[i].clone();
        remove.text.clear();
        bands(&ass_update(result.clone(), vec![remove], 4000, None).unwrap())
    };
    let top = isolated(1);
    let bottom = isolated(0);
    assert!(top.iter().any(|v| *v) && bottom.iter().any(|v| *v));
    assert!(top.iter().zip(bottom).all(|(a, b)| !(*a && b)));
}

#[test]
#[ignore = "Requires CUTROOM_TEST_TOOLS with FFmpeg"]
fn static_sheet_starts_on_the_new_caption_at_an_exact_frame_boundary() {
    let ff = std::path::PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap())
        .join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX));
    let source = SOURCE.split("Dialogue:").next().unwrap().to_owned()
        + "Dialogue: 1,0:00:10.00,0:00:13.80,Caption,,0,0,0,,Before\n"
        + "Dialogue: 1,0:00:13.80,0:00:16.80,Caption,,0,0,0,,After\n";
    let sheet = |frame| super::ass_preview::sheet(&ff, &source, frame, 1).unwrap();
    let first = sheet(414);
    assert_ne!(sheet(413), first);
    assert_eq!(first, sheet(415));
}

#[test]
#[ignore = "Requires CUTROOM_TEST_TOOLS with FFmpeg"]
fn ass_renders_with_scene_time_across_clips() {
    use std::{path::PathBuf, process::Command};
    let temp = tempfile::tempdir().unwrap();
    let ff = PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap())
        .join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX));
    let id = uuid::Uuid::new_v4();
    let store = ProjectStore::new(temp.path().into());
    let mut project: Project = store.load().unwrap();
    project.add_scene("테스트".into());
    let asset=serde_json::from_value(json!({"id":id,"kind":"video","name":"test","extension":"mp4","durationMs":1000,"hasAudio":false})).unwrap();
    project.assets.push(asset);
    let input = project.assets[0].path_in(temp.path());
    assert!(
        Command::new(&ff)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=black:s=160x90:r=30:d=1"
            ])
            .arg(input)
            .status()
            .unwrap()
            .success()
    );
    let s = &mut project.scenes[0].settings;
    s.duration_ms = 2000;
    s.clips = (0..2)
        .map(|_| {
            serde_json::from_value(
                json!({"id":uuid::Uuid::new_v4(),"assetId":id,"inMs":0,"durationMs":1000}),
            )
            .unwrap()
        })
        .collect();
    // Create one two-line caption through the same insertion used by the playhead dialog.
    let empty = super::ass::delete(SOURCE, 500, 1700).unwrap();
    let source = super::ass::insert(&empty, 500, 1700, "I\nMMMMMMMM").unwrap();
    assert_eq!(ass_rows(source.clone()).unwrap().len(), 1);
    s.caption_ass = Some(AssTrack {
        name: "test.ass".into(),
        source: source.clone(),
    });
    let out = temp.path().join("out.mp4");
    render::export(&ff, temp.path(), &project, &out).unwrap();
    let png = temp.path().join("preview.png");
    std::fs::write(
        &png,
        super::ass_preview::sheet(&ff, &source, 36, 1).unwrap(),
    )
    .unwrap();
    let preview = Command::new(&ff)
        .args(["-v", "error", "-i"])
        .arg(png)
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
        .output()
        .unwrap();
    assert_eq!(preview.stdout.len(), 1280 * 720 * 4);
    assert_eq!(preview.stdout[3], 0);
    for (time, visible) in [("0.2", false), ("0.8", true), ("1.2", true), ("1.9", false)] {
        let frame = Command::new(&ff)
            .args(["-v", "error", "-ss", time, "-i"])
            .arg(&out)
            .args([
                "-frames:v",
                "1",
                "-an",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(frame.status.success());
        if time == "1.2" {
            for (pixels, channels) in [(&preview.stdout, 4), (&frame.stdout, 3)] {
                let ink: Vec<_> = pixels
                    .chunks_exact(1280 * channels)
                    .map(|row| {
                        row.chunks_exact(channels)
                            .filter(|p| p.iter().all(|c| *c > 180))
                            .count()
                    })
                    .collect();
                let bands: Vec<_> = ink
                    .split(|&w| w == 0)
                    .filter_map(|r| r.iter().max())
                    .collect();
                assert_eq!(bands.len(), 2, "expected two visible lines: {bands:?}");
                assert!(
                    bands[1] > bands[0],
                    "second line should be wider: {bands:?}"
                );
            }
            let mut diff = 0_u64;
            for (rgb, rgba) in frame
                .stdout
                .chunks_exact(3)
                .zip(preview.stdout.chunks_exact(4))
            {
                for c in 0..3 {
                    let over_black = u16::from(rgba[c]) * u16::from(rgba[3]) / 255;
                    diff += u64::from(u16::from(rgb[c]).abs_diff(over_black));
                }
            }
            assert!(
                diff < 1280 * 720 * 3 * 2,
                "preview/export RGB difference: {diff}"
            );
        }
        let red = frame
            .stdout
            .chunks_exact(3)
            .filter(|p| p[0] > 180 && p[1] < 40)
            .count();
        let white = frame
            .stdout
            .chunks_exact(3)
            .filter(|p| p.iter().all(|c| *c > 180))
            .count();
        assert_eq!((red > 10000, white > 50), (visible, visible), "time {time}");
    }
}

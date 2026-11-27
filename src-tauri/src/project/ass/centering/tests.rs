use super::*;
use crate::project::{ass::ass_rows, ass_preview, ass_tests::SOURCE};

#[test]
fn missing_or_ambiguous_panel_leaves_the_draft_untouched() {
    let line = ass_rows(SOURCE.into()).unwrap()[0].line;
    let missing = SOURCE.replace(",Panel,,", ",Decoration,,");
    assert!(
        placements(Path::new(""), missing, vec![], line, 2000)
            .err()
            .unwrap()
            .contains("없습니다")
    );
    let panel = SOURCE
        .lines()
        .find(|line| line.contains(",Panel,,"))
        .unwrap();
    let duplicate = format!("{SOURCE}{panel}\n");
    assert!(
        placements(Path::new(""), duplicate, vec![], line, 2000)
            .err()
            .unwrap()
            .contains("여러 개")
    );
}

// Inspect the composite preview generated from the serialized ASS, using the
// fixture's red panel and white lettering to measure them independently.
fn assert_preview_centered(ff: &Path, source: &str) {
    let temp = tempfile::tempdir().unwrap();
    let png = temp.path().join("preview.png");
    std::fs::write(&png, ass_preview::sheet(ff, source, 30, 1).unwrap()).unwrap();
    let raw = files::media_command(ff)
        .args(["-v", "error", "-i"])
        .arg(png)
        .args(["-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"])
        .output()
        .unwrap();
    assert_eq!(raw.stdout.len(), WIDTH * HEIGHT * 4);
    let mut bounds = [(WIDTH, HEIGHT, 0, 0); 2];
    for (i, p) in raw.stdout.chunks_exact(4).enumerate() {
        if p[3] < 128 || p[0] < 180 {
            continue;
        }
        let layer = if p[1] < 70 && p[2] < 70 {
            0
        } else if p[1] > 180 && p[2] > 180 {
            1
        } else {
            continue;
        };
        let (left, top, right, bottom) = &mut bounds[layer];
        *left = (*left).min(i % WIDTH);
        *right = (*right).max(i % WIDTH);
        *top = (*top).min(i / WIDTH);
        *bottom = (*bottom).max(i / WIDTH);
    }
    for (l, t, r, b) in bounds {
        assert!(l < r && t < b);
    }
    let [(l, t, r, b), (x, y, u, v)] = bounds;
    assert!(
        (l + r).abs_diff(x + u) <= 4 && (t + b).abs_diff(y + v) <= 4,
        "{bounds:?}"
    );
}

#[test]
#[ignore = "Requires CUTROOM_TEST_TOOLS with FFmpeg"]
fn one_line_hard_break_and_legacy_pair_center_in_the_saved_preview() {
    let ff = std::path::PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap())
        .join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX));
    let source = SOURCE
        .replace("1280", "1920")
        .replace("720", "1080")
        .replace("100,500", "160,780");
    for text in ["한 줄", "첫 줄이 더 긴 자막입니다\n둘째 줄", "예전 윗줄"] {
        let paired = text == "예전 윗줄";
        let source = if paired {
            let upper = source
                .lines()
                .last()
                .unwrap()
                .replace(r"처음,\N둘째 줄", text);
            format!(
                "{}{}\n{}\n",
                source.split_once("Dialogue: 1,").unwrap().0,
                upper,
                upper.replace("600,550", "600,605").replace(text, "아랫줄")
            )
        } else {
            source.replace(r"\pos(600,550)", r"\an7")
        };
        let mut rows = ass_rows(source.clone()).unwrap();
        rows[0].text = text.into();
        let offsets = placements(&ff, source.clone(), rows.clone(), rows[0].line, 2000).unwrap();
        assert_eq!(offsets.len(), if paired { 2 } else { 1 });
        if paired {
            assert!(((offsets[1].center.y - offsets[0].center.y).abs() - 55.0).abs() < 0.01);
        }
        for p in offsets {
            rows.iter_mut().find(|r| r.line == p.line).unwrap().center = Some(p.center);
        }
        let saved = ass_update(source.clone(), rows.clone(), 2000, None).unwrap();
        assert_preview_centered(&ff, &saved);
        assert_eq!(ass_rows(saved.clone()).unwrap()[0].text, text);
        assert!(saved.contains(r"\fad(100,150)"));
        let reopened = ass_rows(saved.clone()).unwrap();
        let again = placements(&ff, saved.clone(), vec![], reopened[0].line, 2000).unwrap();
        let mut edits = reopened;
        for p in again {
            edits.iter_mut().find(|r| r.line == p.line).unwrap().center = Some(p.center);
        }
        assert_eq!(ass_update(saved.clone(), edits, 2000, None).unwrap(), saved);
    }
}

use super::{Event, Row, new_rows::style_metrics};

// These comments survive saving and importing the ASS. Libass ignores them
// and stacks bottom-aligned Dialogue events upwards in their physical order.
pub(super) const BEGIN: &str = "; Cutroom bottom-up caption rows begin";
pub(super) const END: &str = "; Cutroom bottom-up caption rows end";

pub(super) fn bottom_flow(source: &str, event: &Event) -> bool {
    if event.effect || event.drawing {
        return false;
    }
    let mut alignment = None;
    let text = event.fields.last().copied().unwrap_or_default();
    for tag in text
        .split('{')
        .skip(1)
        .filter_map(|s| s.split_once('}'))
        .flat_map(|(tags, _)| tags.split('\\').skip(1))
    {
        if ["pos(", "move(", "org(", "t(", "clip(", "iclip(", "r"]
            .iter()
            .any(|prefix| tag.starts_with(prefix))
        {
            return false;
        }
        if alignment.is_none() {
            // Both ASS \an and legacy \a use 1, 2, 3 for bottom alignment.
            alignment = tag
                .strip_prefix("an")
                .or_else(|| tag.strip_prefix('a'))
                .and_then(|v| v.trim().parse::<u8>().ok())
                .filter(|v| *v > 0);
        }
    }
    (1..=3).contains(&alignment.unwrap_or_else(|| style_metrics(source, event).2))
}

pub(super) fn groups(source: &str) -> impl Iterator<Item = std::ops::RangeInclusive<usize>> + '_ {
    let mut start = None;
    source.lines().enumerate().filter_map(move |(line, text)| {
        match text.trim() {
            BEGIN => start = Some(line),
            END => return start.take().map(|first| first..=line),
            header if header.starts_with('[') && header.ends_with(']') => start = None,
            _ => {}
        }
        None
    })
}

pub(super) fn editor_order(source: &str, rows: &mut [Row]) {
    let mut cursor = 0;
    for group in groups(source) {
        let remaining = &mut rows[cursor..];
        let start = remaining.partition_point(|r| r.line <= *group.start());
        let end = remaining.partition_point(|r| r.line < *group.end());
        remaining[start..end].reverse();
        cursor += end;
    }
}

#[cfg(test)]
mod tests {
    use super::super::{NewRow, ass_rows, ass_update};

    const SOURCE: &str = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 640
PlayResY: 360
[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Caption,Arial,40,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,20,20,20,1
[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:02.00,Caption,,0,0,0,,I
Dialogue: 0,0:00:02.00,0:00:04.00,Caption,,0,0,0,,Other cue
"#;

    fn add(source: &str, template: usize, texts: &[&str]) -> String {
        let line = ass_rows(source.to_owned()).unwrap()[template].line;
        let additions = texts
            .iter()
            .map(|t| NewRow {
                template_line: line,
                text: (*t).to_owned(),
            })
            .collect();
        ass_update(source.to_owned(), vec![], 4000, Some(additions)).unwrap()
    }

    fn four_rows() -> String {
        let first_save = add(SOURCE, 0, &["MMM", "MMMMMMM"]);
        add(&first_save, 1, &["MMMMM"])
    }

    #[test]
    fn bottom_flow_keeps_editor_order_and_independent_edits_after_reopening() {
        let saved = four_rows();
        let rows = ass_rows(saved.clone()).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["I", "MMM", "MMMMM", "MMMMMMM", "Other cue"]
        );
        assert!(
            rows[..4]
                .iter()
                .all(|r| (r.start_ms, r.end_ms) == (0, 2000))
        );
        let physical = super::super::events(&saved).unwrap();
        assert_eq!(
            physical.iter().map(|e| e.body).collect::<Vec<_>>(),
            ["MMMMMMM", "MMMMM", "MMM", "I", "Other cue"]
        );
        assert_eq!(saved.matches(super::BEGIN).count(), 1);
        assert_eq!(saved.matches(super::END).count(), 1);
        assert_eq!(saved.lines().last(), SOURCE.lines().last());
        let mut edited = rows[2].clone();
        edited.text = "changed".to_owned();
        edited.end_ms = 750;
        let updated = ass_update(saved, vec![edited], 4000, None).unwrap();
        let rows = ass_rows(updated).unwrap();
        assert_eq!((&*rows[2].text, rows[2].end_ms), ("changed", 750));
        assert_eq!((&*rows[3].text, rows[3].end_ms), ("MMMMMMM", 2000));
    }

    #[test]
    fn top_aligned_flow_keeps_authored_ass_order() {
        let source = SOURCE.replace(",0,0,2,20,20,20,1", ",0,0,8,20,20,20,1");
        let saved = add(&source, 0, &["added"]);
        assert!(!saved.contains(super::BEGIN));
        let physical = super::super::events(&saved).unwrap();
        assert_eq!(
            physical.iter().map(|e| e.body).collect::<Vec<_>>(),
            ["I", "added", "Other cue"]
        );
    }

    #[test]
    fn inserting_a_new_timed_caption_leaves_the_bottom_flow_group() {
        let source = add(SOURCE, 0, &["second"]);
        let split = super::super::insert(&source, 1000, 4000, "next").unwrap();
        let rows = ass_rows(split.clone()).unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| (&*r.text, r.start_ms, r.end_ms))
                .collect::<Vec<_>>(),
            [
                ("I", 0, 1000),
                ("second", 0, 1000),
                ("next", 1000, 2000),
                ("Other cue", 2000, 4000)
            ]
        );
        let group = super::groups(&split).next().unwrap();
        assert!(rows[2].line > *group.end());
    }

    #[test]
    #[ignore = "Run with CUTROOM_TEST_TOOLS pointing to a directory containing FFmpeg"]
    fn bottom_flow_renders_added_rows_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = std::path::PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap())
            .join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX));
        let render = |source| {
            std::fs::write(dir.path().join("caption.ass"), source).unwrap();
            let frame = std::process::Command::new(&ffmpeg)
                .current_dir(dir.path())
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=s=640x360:r=1:d=1",
                    "-vf",
                    "ass=caption.ass",
                    "-frames:v",
                    "1",
                    "-pix_fmt",
                    "gray",
                    "-f",
                    "rawvideo",
                    "-",
                ])
                .output()
                .unwrap();
            assert!(
                frame.status.success(),
                "{}",
                String::from_utf8_lossy(&frame.stderr)
            );
            assert_eq!(frame.stdout.len(), 640 * 360);
            let mut widths = vec![];
            let mut width = 0;
            for line in frame.stdout.chunks_exact(640) {
                if let (Some(left), Some(right)) = (
                    line.iter().position(|&p| p > 128),
                    line.iter().rposition(|&p| p > 128),
                ) {
                    width = width.max(right + 1 - left);
                } else if width > 0 {
                    widths.push(width);
                    width = 0;
                }
            }
            widths
        };
        // Each successive text, I / MMM / MMMMM / MMMMMMM, is wider.
        let widths = render(four_rows());
        assert_eq!(widths.len(), 4, "{widths:?}");
        assert!(
            widths.windows(2).all(|pair| pair[0] < pair[1]),
            "{widths:?}"
        );
        let wrapped = add(SOURCE, 0, &["MMMM MMMM MMMM MMMM MMMM MMMM MMMM MMMM"]);
        let widths = render(wrapped);
        assert!(
            widths.len() >= 3 && widths[0] < 30 && widths[1..].iter().all(|&w| w > 100),
            "{widths:?}"
        );
    }
}

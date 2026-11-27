use super::{Event, Row, ass_update, events, multiline_layout, new_rows, render_filter, rewrite};
use crate::project::{ProjectStore, files, run_io};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};

#[derive(Clone, Copy, Deserialize, Serialize)]
pub(crate) struct Point {
    x: f64,
    y: f64,
}

#[derive(Serialize)]
pub(crate) struct Placement {
    line: usize,
    center: Point,
}

// Keep typography and fades. Replace only the alignment and fixed anchor.
pub(super) fn prefix(original: &str, point: Point) -> Result<String, String> {
    if !point.x.is_finite() || !point.y.is_finite() || original.contains(r"\move(") {
        return Err("움직이는 자막은 고정된 위치에 맞출 수 없습니다.".into());
    }
    let mut rest = original.to_owned();
    if let Some((range, args, _)) = new_rows::position(original) {
        if args.len() != 2 {
            return Err("자막의 위치 지정을 확인해 주세요.".into());
        }
        rest.replace_range(range.start - 5..range.end + 1, "");
    }
    // Remove both modern and SSA legacy alignment tags, without touching \alpha.
    for tag in [r"\an", r"\a"] {
        let mut at = 0;
        while let Some(i) = rest[at..].find(tag) {
            let start = at + i;
            let digits = start + tag.len();
            let n = rest[digits..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            if n > 0 {
                rest.replace_range(start..digits + n, "");
                at = start;
            } else {
                at = digits;
            }
        }
    }
    Ok(format!(
        r"{{\an5\pos({:.3},{:.3})}}{}",
        point.x,
        point.y,
        rest.replace("{}", "")
    ))
}

fn script_size(source: &str) -> Point {
    let mut x = None;
    let mut y = None;
    let mut info = false;
    for line in source.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.starts_with('[') {
            info = line.eq_ignore_ascii_case("[Script Info]");
        }
        if !info {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|&v| v > 0)
                .map(f64::from);
            if key.trim().eq_ignore_ascii_case("PlayResX") {
                x = value;
            }
            if key.trim().eq_ignore_ascii_case("PlayResY") {
                y = value;
            }
        }
    }
    // libass infers an omitted dimension from a 4:3 script (except 1280x1024).
    match (x, y) {
        (Some(x), Some(y)) => Point { x, y },
        (Some(x), None) => Point {
            x,
            y: if x == 1280.0 {
                1024.0
            } else {
                (x * 0.75).floor()
            },
        },
        (None, Some(y)) => Point {
            x: if y == 1024.0 {
                1280.0
            } else {
                (y * 4.0 / 3.0).floor()
            },
            y,
        },
        _ => Point { x: 384.0, y: 288.0 },
    }
}

// Preserve every header, style and Format declaration around the retained events.
fn isolated(source: &str, lines: &HashSet<usize>, keep: &[(usize, String)]) -> String {
    source
        .lines()
        .enumerate()
        .filter_map(|(i, raw)| {
            if !lines.contains(&i) {
                return Some(raw);
            }
            keep.iter()
                .find(|(line, _)| *line == i)
                .map(|(_, v)| v.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

fn visible_center(ffmpeg: &Path, dir: &Path, source: &str, at: u64) -> Result<Point, String> {
    std::fs::write(dir.join("scene.ass"), source).map_err(|e| e.to_string())?;
    let filter = format!("{},alphaextract", render_filter(at * 30, true));
    let output = files::media_command(ffmpeg)
        .current_dir(dir)
        .args([
            "-nostdin",
            "-v",
            "error",
            "-filter_threads",
            "2",
            "-f",
            "lavfi",
            "-i",
            &format!("color=black@0:s={WIDTH}x{HEIGHT}:r=30,format=yuva420p"),
            "-vf",
            &filter,
            "-frames:v",
            "1",
            "-threads",
            "1",
            "-pix_fmt",
            "gray",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() || output.stdout.len() != WIDTH * HEIGHT {
        return Err(format!(
            "자막을 그릴 수 없습니다. {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let (mut left, mut top, mut right, mut bottom) = (WIDTH, HEIGHT, 0, 0);
    for (y, row) in output.stdout.chunks_exact(WIDTH).enumerate() {
        if let Some(x) = row.iter().position(|&a| a > 0) {
            left = left.min(x);
            right = right.max(row.iter().rposition(|&a| a > 0).unwrap());
            top = top.min(y);
            bottom = y;
        }
    }
    if top == HEIGHT {
        return Err("구간 중간에 보이는 자막 또는 배경이 없습니다.".into());
    }
    Ok(Point {
        x: (right + left) as f64 / 2.0,
        y: (bottom + top) as f64 / 2.0,
    })
}

fn placements(
    ffmpeg: &Path,
    source: String,
    edits: Vec<Row>,
    line: usize,
    duration: u64,
) -> Result<Vec<Placement>, String> {
    // Deletions can split panel events and renumber physical lines. Surviving
    // text events retain their order, including after a two-row merge.
    let deleted: HashSet<_> = edits
        .iter()
        .filter(|r| r.text.trim().is_empty())
        .map(|r| r.line)
        .collect();
    let originals: Vec<_> = events(&source)?
        .iter()
        .filter(|e| !e.drawing && !deleted.contains(&e.line))
        .map(|e| e.line)
        .collect();
    let compiled = ass_update(source, edits, duration, None)?;
    let parsed = events(&compiled)?;
    let captions: Vec<_> = parsed.iter().filter(|e| !e.drawing).collect();
    let index = originals
        .iter()
        .position(|&i| i == line)
        .ok_or("편집할 자막이 없습니다.")?;
    let target = captions.get(index).ok_or("편집할 자막이 없습니다.")?;
    let mut panels: Vec<_> = parsed
        .iter()
        .filter(|p| p.panel && p.from <= target.from && target.to <= p.to)
        .collect();
    panels.sort_by_key(|p| p.to - p.from);
    let panel = panels
        .first()
        .ok_or("이 자막 구간에 맞는 ASS 배경(Panel)이 없습니다.")?;
    if panels
        .get(1)
        .is_some_and(|p| p.to - p.from == panel.to - panel.from)
    {
        return Err("겹치는 ASS 배경이 여러 개여서 중앙을 정할 수 없습니다.".into());
    }
    let size = script_size(&compiled);
    let anchor = Point {
        x: size.x / 2.0,
        y: size.y / 2.0,
    };
    let mut group: Vec<(usize, &Event, Point)> = vec![(line, target, anchor)];
    if let Some((peer_line, peer_y)) =
        multiline_layout::paired_slot(&compiled, &parsed, target, |_| true)
    {
        let (_, _, y) = new_rows::position(target.prefix).unwrap();
        if let Some(i) = captions.iter().position(|e| e.line == peer_line) {
            group.push((
                originals[i],
                captions[i],
                Point {
                    x: anchor.x,
                    y: anchor.y + peer_y - y,
                },
            ));
        }
    }
    let mut samples = Vec::new();
    for (_, event, pos) in &group {
        if event.effect || event.body.contains(r"\pos(") || event.body.contains(r"\move(") {
            return Err("자막에 별도의 위치 효과가 있어 중앙을 정할 수 없습니다.".into());
        }
        let text = format!("{}{}", prefix(event.prefix, *pos)?, event.body);
        samples.push((event.line, rewrite(event, event.from, event.to, &text)));
    }
    let lines = parsed.iter().map(|e| e.line).collect();
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let at = target.from + (target.to - target.from) / 2;
    let panel_script = isolated(
        &compiled,
        &lines,
        &[(
            panel.line,
            rewrite(panel, panel.from, panel.to, panel.fields.last().unwrap()),
        )],
    );
    let background = visible_center(ffmpeg, temp.path(), &panel_script, at)?;
    let ink = visible_center(
        ffmpeg,
        temp.path(),
        &isolated(&compiled, &lines, &samples),
        at,
    )?;
    let dx = (background.x - ink.x) * size.x / WIDTH as f64;
    let dy = (background.y - ink.y) * size.y / HEIGHT as f64;
    Ok(group
        .into_iter()
        .map(|(line, _, p)| Placement {
            line,
            center: Point {
                x: p.x + dx,
                y: p.y + dy,
            },
        })
        .collect())
}

#[tauri::command]
pub(crate) async fn ass_center_on_panel(
    store: ProjectStore,
    source: String,
    edits: Vec<Row>,
    line: usize,
    duration_ms: u64,
) -> Result<Vec<Placement>, String> {
    let root = store.app_data.clone();
    run_io(move || {
        let ff = files::find_tool(&root, "ffmpeg").ok_or("FFmpeg를 찾을 수 없습니다.")?;
        placements(&ff, source, edits, line, duration_ms)
    })
    .await
}

#[cfg(test)]
mod tests;

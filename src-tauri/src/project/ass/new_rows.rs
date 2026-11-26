use super::{Event, Row, events, join_lines, rewrite};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewRow {
    pub template_line: usize,
    pub text: String,
}

// Fixed-position ASS events opt out of libass collision handling. Shift both
// ends of a moving caption together so its motion and timing stay intact.
fn position(prefix: &str) -> Option<(std::ops::Range<usize>, Vec<&str>, f64)> {
    for tag in [r"\pos(", r"\move("] {
        if let Some(start) = prefix.find(tag) {
            let start = start + tag.len();
            let (args, _) = prefix[start..].split_once(')')?;
            let parts: Vec<_> = args.split(',').map(str::trim).collect();
            let y = parts.get(1)?.parse().ok()?;
            return Some((start..start + args.len(), parts, y));
        }
    }
    None
}

fn style_metrics(source: &str, event: &Event) -> (f64, f64, u8) {
    let name = event
        .style_index
        .map(|i| event.fields[i].trim())
        .unwrap_or("Default");
    let mut style_section = false;
    let mut columns = Vec::new();
    for raw in source.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            style_section = line.eq_ignore_ascii_case("[V4+ Styles]");
        }
        if !style_section {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let fields: Vec<_> = value.split(',').map(str::trim).collect();
        if key.eq_ignore_ascii_case("Format") {
            columns = fields;
        } else if key.eq_ignore_ascii_case("Style") {
            let field = |key: &str| {
                columns
                    .iter()
                    .position(|c| c.eq_ignore_ascii_case(key))
                    .and_then(|i| fields.get(i))
                    .copied()
            };
            if field("Name") == Some(name) {
                return (
                    field("Fontsize")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(20.0),
                    field("ScaleY")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(100.0),
                    field("Alignment").and_then(|v| v.parse().ok()).unwrap_or(2),
                );
            }
        }
    }
    (20.0, 100.0, 2)
}

fn vertical_bounds(source: &str, event: &Event, text: &str, args: &[&str], y: f64) -> (f64, f64) {
    let (size, scale, align) = style_metrics(source, event);
    let override_number = |name: &str| {
        event
            .prefix
            .split('\\')
            .filter_map(|tag| {
                tag.strip_prefix(name)?
                    .trim_end_matches('}')
                    .parse::<f64>()
                    .ok()
            })
            .last()
    };
    let line_count = 1 + text.matches(r"\N").count() + text.matches('\n').count();
    let height =
        (override_number("fs").unwrap_or(size) * override_number("fscy").unwrap_or(scale) * 0.012)
            .max(1.0)
            * line_count as f64;
    let top = match override_number("an").unwrap_or(align as f64) as u8 {
        7..=9 => 0.0,
        4..=6 => -height / 2.0,
        _ => -height,
    };
    let end_y = args.get(3).and_then(|v| v.parse::<f64>().ok()).unwrap_or(y);
    (y.min(end_y) + top, y.max(end_y) + top + height)
}

pub(super) fn expand(
    source: &str,
    mut edits: Vec<Row>,
    additions: Vec<NewRow>,
) -> Result<(String, Vec<Row>), String> {
    let parsed = events(source)?;
    let height = source
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find_map(|(k, v)| {
            k.trim()
                .eq_ignore_ascii_case("PlayResY")
                .then(|| v.trim().parse::<f64>().ok())
                .flatten()
        })
        .unwrap_or(288.0);
    let mut occupied = Vec::new();
    let mut by_line = HashMap::new();
    for event in parsed.iter().filter(|e| !e.drawing) {
        let edit = edits.iter().find(|r| r.line == event.line);
        let (from, to) = edit.map_or((event.from, event.to), |r| (r.start_ms, r.end_ms));
        by_line.insert(event.line, (event, from, to));
        if !edit.is_some_and(|r| r.text.trim().is_empty())
            && let Some((_, args, y)) = position(event.prefix)
        {
            let (top, bottom) = vertical_bounds(
                source,
                event,
                edit.map_or(event.body, |r| &r.text),
                &args,
                y,
            );
            occupied.push((from, to, top, bottom));
        }
    }
    let mut inserts: HashMap<usize, Vec<String>> = HashMap::new();
    for addition in additions {
        let &(template, from, to) = by_line
            .get(&addition.template_line)
            .ok_or("추가할 줄의 기준 자막을 찾을 수 없습니다.")?;
        if addition.text.trim().is_empty() || addition.text.contains(['\r', '\0']) {
            return Err("추가할 줄의 문구를 입력해 주세요.".into());
        }
        let mut prefix = template.prefix.to_owned();
        if let Some((range, args, y)) = position(template.prefix) {
            let (top, bottom) = vertical_bounds(source, template, &addition.text, &args, y);
            let overlapping: Vec<_> = occupied
                .iter()
                .filter(|&&(a, b, _, _)| a < to && from < b)
                .collect();
            let delta = std::iter::once(0.0)
                .chain(
                    overlapping
                        .iter()
                        .flat_map(|&&(_, _, a, b)| [b - top, a - bottom]),
                )
                .filter(|d| {
                    top + d >= 0.0
                        && bottom + d <= height
                        && overlapping
                            .iter()
                            .all(|&&(_, _, a, b)| bottom + d <= a + 0.001 || b <= top + d + 0.001)
                })
                .min_by(|a, b| {
                    (*a < 0.0)
                        .cmp(&(*b < 0.0))
                        .then_with(|| a.abs().total_cmp(&b.abs()))
                })
                .ok_or("화면에 새 줄을 배치할 공간이 부족합니다.")?;
            let new_y = y + delta;
            let mut args: Vec<_> = args.into_iter().map(str::to_owned).collect();
            args[1] = new_y.to_string();
            if args.len() >= 4
                && let Ok(end_y) = args[3].parse::<f64>()
            {
                args[3] = (end_y + new_y - y).to_string();
            }
            prefix.replace_range(range, &args.join(","));
            occupied.push((from, to, top + delta, bottom + delta));
        }
        let text = prefix + &addition.text.replace('\n', r"\N");
        inserts
            .entry(template.line)
            .or_default()
            .push(rewrite(template, from, to, &text));
    }
    // Keep each addition in its template's Events/Format section. Remap every
    // existing edit once, including deletions, before applying the transaction.
    let mut lines = Vec::new();
    let mut original_lines = Vec::new();
    for (index, line) in source.lines().enumerate() {
        original_lines.push(lines.len());
        lines.push(line.to_owned());
        if let Some(new) = inserts.remove(&index) {
            lines.extend(new);
        }
    }
    for edit in &mut edits {
        edit.line = *original_lines
            .get(edit.line)
            .ok_or("수정할 자막을 찾을 수 없습니다.")?;
    }
    Ok((join_lines(source, lines)?, edits))
}

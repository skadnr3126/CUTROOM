use super::{Event, Row, events, flow_rows, join_lines, rewrite};
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

pub(super) fn style_metrics(source: &str, event: &Event) -> (f64, f64, u8) {
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

fn visible_lines(text: &str) -> usize {
    let mut text = text.trim_end();
    while let Some(rest) = text.strip_suffix(r"\N") {
        text = rest.trim_end();
    }
    1 + text.matches(r"\N").count() + text.matches('\n').count()
}

fn vertical_bounds(source: &str, event: &Event, lines: usize, args: &[&str], y: f64) -> (f64, f64) {
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
    let height =
        (override_number("fs").unwrap_or(size) * override_number("fscy").unwrap_or(scale) * 0.012)
            .max(1.0)
            * lines as f64;
    let top = match override_number("an").unwrap_or(align as f64) as u8 {
        7..=9 => 0.0,
        4..=6 => -height / 2.0,
        _ => -height,
    };
    let end_y = args.get(3).and_then(|v| v.parse::<f64>().ok()).unwrap_or(y);
    (y.min(end_y) + top, y.max(end_y) + top + height)
}

// Only borrow spacing between static, single-line captions in the same column
// with the same style and overrides. Keep drawings and moving effects separate.
#[derive(Clone, Copy, PartialEq)]
struct Layout<'a> {
    style: &'a str,
    before_pos: &'a str,
    after_pos: &'a str,
    x: f64,
}

fn layout<'a>(
    event: &Event<'a>,
    range: &std::ops::Range<usize>,
    args: &[&str],
    text: &str,
    lines: usize,
) -> Option<Layout<'a>> {
    if lines != 1 || event.effect || text.contains(r"{\") || args.len() != 2 {
        return None;
    }
    Some(Layout {
        style: event
            .style_index
            .map_or("Default", |i| event.fields[i].trim()),
        before_pos: event.prefix[..range.start].strip_suffix(r"\pos(")?,
        after_pos: &event.prefix[range.end..],
        x: args[0].parse().ok()?,
    })
}

struct PlacedRow<'a> {
    template_line: usize,
    from: u64,
    to: u64,
    initial_y: f64,
    y: f64,
    top: f64,
    bottom: f64,
    layout: Option<Layout<'a>>,
}

impl<'a> PlacedRow<'a> {
    fn from_event(source: &str, event: &Event<'a>, text: &str, from: u64, to: u64) -> Option<Self> {
        let (range, args, y) = position(event.prefix)?;
        let lines = visible_lines(text);
        let (top, bottom) = vertical_bounds(source, event, lines, &args, y);
        Some(Self {
            template_line: event.line,
            from,
            to,
            initial_y: y,
            y,
            top,
            bottom,
            layout: layout(event, &range, &args, text, lines),
        })
    }

    fn shift(&mut self, delta: f64) {
        self.y += delta;
        self.top += delta;
        self.bottom += delta;
    }

    fn overlaps(&self, from: u64, to: u64) -> bool {
        self.from < to && from < self.to
    }

    fn fits(&self, delta: f64, height: f64) -> bool {
        self.top + delta >= -0.001 && self.bottom + delta <= height + 0.001
    }

    fn clears(&self, other: &Self, delta: f64) -> bool {
        self.bottom + delta <= other.top + 0.001 || other.bottom <= self.top + delta + 0.001
    }

    fn authored_delta(
        &self,
        originals: &[Self],
        overlapping: &[&Self],
        height: f64,
        last_y: f64,
    ) -> Option<f64> {
        let layout = self.layout?;
        let mut groups = HashMap::<_, Vec<f64>>::new();
        for row in originals {
            if row.layout == Some(layout) && !row.overlaps(self.from, self.to) {
                groups
                    .entry((row.from, row.to))
                    .or_default()
                    .push(row.initial_y);
            }
        }
        let mut candidates = Vec::new();
        for ((from, to), anchors) in groups {
            if !anchors.contains(&self.y) {
                continue;
            }
            let gap = from
                .saturating_sub(self.to)
                .max(self.from.saturating_sub(to));
            for &y in anchors.iter().filter(|&&y| y > last_y) {
                let delta = y - self.y;
                // An authored pair can be closer than our estimated line boxes.
                // Only waive that estimate for distinct slots in this exact layout.
                if self.fits(delta, height)
                    && overlapping.iter().all(|row| {
                        self.clears(row, delta)
                            || (row.layout == Some(layout)
                                && row.y != y
                                && anchors.contains(&row.y))
                    })
                {
                    candidates.push((gap, delta));
                }
            }
        }
        candidates
            .into_iter()
            .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)))
            .map(|(_, d)| d)
    }

    fn place_below(
        &mut self,
        occupied: &mut [Self],
        original_count: usize,
        height: f64,
    ) -> Result<(), String> {
        let overlapping: Vec<_> = occupied
            .iter()
            .filter(|r| r.overlaps(self.from, self.to))
            .collect();
        let last_y = overlapping
            .iter()
            .filter(|r| r.template_line == self.template_line)
            .map(|r| r.y)
            .fold(self.y, f64::max);
        let delta = self
            .authored_delta(&occupied[..original_count], &overlapping, height, last_y)
            .or_else(|| {
                std::iter::once(0.0)
                    .chain(overlapping.iter().map(|r| r.bottom - self.top))
                    .filter(|&d| {
                        self.y + d >= last_y
                            && self.fits(d, height)
                            && overlapping.iter().all(|r| self.clears(r, d))
                    })
                    .min_by(f64::total_cmp)
            });
        if let Some(delta) = delta {
            self.shift(delta);
            return Ok(());
        }

        // At the bottom edge, lift the simultaneous stack just enough to fit
        // the new row below it. Other captions stay at their authored positions.
        let (line, layout, interval) = (self.template_line, self.layout, (self.from, self.to));
        let in_stack = |r: &Self| {
            r.template_line == line
                || (layout.is_some() && r.layout == layout && (r.from, r.to) == interval)
        };
        let (stack, obstacles): (Vec<_>, Vec<_>) =
            overlapping.into_iter().partition(|r| in_stack(r));
        let below = stack
            .iter()
            .map(|r| r.bottom - self.top)
            .fold(0.0, f64::max);
        self.shift(below);
        let edge = (height - self.bottom).min(0.0);
        let stack = stack
            .into_iter()
            .chain(std::iter::once(&*self))
            .collect::<Vec<_>>();
        let shift = std::iter::once(edge)
            .chain(
                stack
                    .iter()
                    .flat_map(|r| obstacles.iter().map(move |o| o.top - r.bottom)),
            )
            .filter(|&d| {
                d <= edge
                    && stack
                        .iter()
                        .all(|r| r.fits(d, height) && obstacles.iter().all(|o| r.clears(o, d)))
            })
            .max_by(f64::total_cmp)
            .ok_or("기준 자막 아래에 새 줄을 배치할 공간이 부족합니다.")?;
        for row in occupied.iter_mut().filter(|r| in_stack(r)) {
            row.shift(shift);
        }
        self.shift(shift);
        Ok(())
    }

    // A reopened cue can already have a second or third row. Insert between
    // its rows in the same order as F2, reusing the slots we just made fit.
    fn insert_after_template(&mut self, occupied: &mut [Self]) {
        let Some(layout) = self.layout else { return };
        let Some(after_y) = occupied
            .iter()
            .filter(|r| r.template_line == self.template_line)
            .map(|r| r.y)
            .max_by(f64::total_cmp)
        else {
            return;
        };
        let mut siblings: Vec<_> = occupied
            .iter_mut()
            .filter(|r| {
                r.layout == Some(layout)
                    && (r.from, r.to) == (self.from, self.to)
                    && after_y < r.y
                    && r.y < self.y
            })
            .collect();
        siblings.sort_by(|a, b| b.y.total_cmp(&a.y));
        for sibling in siblings {
            let shift = self.y - sibling.y;
            sibling.shift(shift);
            self.shift(-shift);
        }
    }
}

fn placed_text(event: &Event, text: &str, row: Option<&PlacedRow>) -> String {
    let mut prefix = event.prefix.to_owned();
    if let Some(row) = row.filter(|r| r.y != r.initial_y)
        && let Some((range, args, _)) = position(event.prefix)
    {
        let mut args: Vec<_> = args.into_iter().map(str::to_owned).collect();
        args[1] = row.y.to_string();
        if let Some(end) = args.get_mut(3)
            && let Ok(end_y) = end.parse::<f64>()
        {
            *end = (end_y + (row.y - row.initial_y)).to_string();
        }
        prefix.replace_range(range, &args.join(","));
    }
    prefix + text
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
    let mut placed_originals = HashMap::new();
    for event in parsed.iter().filter(|e| !e.drawing) {
        let edit = edits.iter().find(|r| r.line == event.line);
        let (from, to) = edit.map_or((event.from, event.to), |r| (r.start_ms, r.end_ms));
        by_line.insert(event.line, (event, from, to));
        let text = edit.map_or(event.body, |r| &r.text);
        if !text.trim().is_empty()
            && let Some(row) = PlacedRow::from_event(source, event, text, from, to)
        {
            placed_originals.insert(event.line, occupied.len());
            occupied.push(row);
        }
    }
    let original_count = occupied.len();
    let mut inserts: HashMap<usize, Vec<(usize, Option<usize>)>> = HashMap::new();
    for (index, addition) in additions.iter().enumerate() {
        let &(template, from, to) = by_line
            .get(&addition.template_line)
            .ok_or("추가할 줄의 기준 자막을 찾을 수 없습니다.")?;
        if addition.text.trim().is_empty() || addition.text.contains(['\r', '\0']) {
            return Err("추가할 줄의 문구를 입력해 주세요.".into());
        }
        let placed = if let Some(mut row) =
            PlacedRow::from_event(source, template, &addition.text, from, to)
        {
            if let Some(&i) = placed_originals.get(&template.line) {
                row.shift(occupied[i].y - row.y);
            }
            row.place_below(&mut occupied, original_count, height)?;
            row.insert_after_template(&mut occupied);
            occupied.push(row);
            Some(occupied.len() - 1)
        } else {
            None
        };
        inserts
            .entry(template.line)
            .or_default()
            .push((index, placed));
    }
    // Keep each addition in its template's Events/Format section. Remap every
    // existing edit once, including deletions, before applying the transaction.
    let mut lines = Vec::new();
    let mut original_lines = Vec::new();
    let bottom_groups: Vec<_> = flow_rows::groups(source).collect();
    for (index, line) in source.lines().enumerate() {
        let mut added = Vec::new();
        let mut wrap_group = false;
        if let Some(new) = inserts.remove(&index) {
            let (template, from, to) = by_line[&index];
            for (i, placed) in new {
                let text = placed_text(
                    template,
                    &additions[i].text.replace('\n', r"\N"),
                    placed.map(|n| &occupied[n]),
                );
                added.push(rewrite(template, from, to, &text));
            }
            // libass stacks later unpositioned bottom-aligned Dialogue rows upwards.
            // Store this group in draw order, keeping its reading order in the editor.
            let in_bottom_group = bottom_groups.iter().any(|g| g.contains(&index));
            wrap_group = !in_bottom_group && flow_rows::bottom_flow(source, template);
            if wrap_group {
                lines.push(flow_rows::BEGIN.to_owned());
            }
            if wrap_group || in_bottom_group {
                lines.extend(added.drain(..).rev());
            }
        }
        original_lines.push(lines.len());
        let placed = placed_originals.get(&index).map(|&i| &occupied[i]);
        if let Some(row) = placed.filter(|r| r.initial_y != r.y) {
            let (event, _, _) = by_line[&index];
            lines.push(rewrite(
                event,
                event.from,
                event.to,
                &placed_text(event, event.body, Some(row)),
            ));
        } else {
            lines.push(line.to_owned());
        }
        lines.extend(added);
        if wrap_group {
            lines.push(flow_rows::END.to_owned());
        }
    }
    for edit in &mut edits {
        edit.line = *original_lines
            .get(edit.line)
            .ok_or("수정할 자막을 찾을 수 없습니다.")?;
    }
    Ok((join_lines(source, lines)?, edits))
}

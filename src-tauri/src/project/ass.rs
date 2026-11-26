use serde::{Deserialize, Serialize};
mod flow_rows;
mod new_rows;
pub(crate) use new_rows::NewRow;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AssTrack {
    pub name: String,
    pub source: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    pub line: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

struct Event<'a> {
    style_only: bool,
    line: usize,
    fields: Vec<&'a str>,
    start: usize,
    end: usize,
    style_index: Option<usize>,
    prefix: &'a str,
    body: &'a str,
    from: u64,
    to: u64,
    drawing: bool,
    panel: bool,
    effect: bool,
}

fn time(s: &str) -> Option<u64> {
    let (hour, rest) = s.trim().split_once(':')?;
    let (minute, rest) = rest.split_once(':')?;
    let (second, cs) = rest.split_once('.')?;
    let (h, m, s, c) = (
        hour.parse::<u32>().ok()?,
        minute.parse::<u32>().ok()?,
        second.parse::<u32>().ok()?,
        cs.parse::<u32>().ok()?,
    );
    if cs.len() != 2 || m >= 60 || s >= 60 || c >= 100 {
        return None;
    }
    Some(u64::from(h) * 3600000 + u64::from(m * 60000 + s * 1000 + c * 10))
}

fn stamp(ms: u64) -> String {
    format!(
        "{}:{:02}:{:02}.{:02}",
        ms / 3600000,
        ms / 60000 % 60,
        ms / 1000 % 60,
        ms / 10 % 100
    )
}

// Keep the final caption's layout in ASS comments so an empty track can be reused.
const STYLE_TEMPLATE: &str = "; Cutroom caption style";

fn all_events(source: &str) -> Result<Vec<Event<'_>>, String> {
    if source.len() > 2_000_000 || source.contains('\0') {
        return Err("ASS 파일의 크기 또는 인코딩을 확인해 주세요.".into());
    }
    let mut section = "";
    let mut styles = false;
    let mut format = Vec::new();
    let mut result = Vec::new();
    for (line, raw) in source.trim_start_matches('\u{feff}').lines().enumerate() {
        let t = raw.trim();
        if t.starts_with('[') && t.ends_with(']') {
            section = t;
            continue;
        }
        let Some((key, value)) = raw.trim_start().split_once(':') else {
            continue;
        };
        if section.eq_ignore_ascii_case("[V4+ Styles]") && key.eq_ignore_ascii_case("Style") {
            styles = true;
        }
        if !section.eq_ignore_ascii_case("[Events]") {
            continue;
        }
        if key.eq_ignore_ascii_case("Format") {
            format = value
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .collect();
            continue;
        }
        let style_only = key == STYLE_TEMPLATE;
        if !style_only && !key.eq_ignore_ascii_case("Dialogue") {
            continue;
        }
        let bad = || format!("ASS {}행의 대사 형식이나 시간을 확인해 주세요.", line + 1);
        if format.last().map(String::as_str) != Some("text") {
            return Err(bad());
        }
        let fields: Vec<_> = value.trim_start().splitn(format.len(), ',').collect();
        if fields.len() != format.len() {
            return Err(bad());
        }
        let start = format.iter().position(|s| s == "start").ok_or_else(bad)?;
        let end = format.iter().position(|s| s == "end").ok_or_else(bad)?;
        let from = time(fields[start]).ok_or_else(bad)?;
        let to = time(fields[end]).ok_or_else(bad)?;
        if from >= to {
            return Err(bad());
        }
        let text = *fields.last().ok_or_else(bad)?;
        let mut body = text;
        while body.starts_with('{') {
            let Some((_, tail)) = body.split_once('}') else {
                break;
            };
            body = tail;
        }
        let prefix = &text[..text.len() - body.len()];
        let drawing = prefix.split('\\').any(|t| {
            t.strip_prefix('p')
                .and_then(|v| v.chars().next())
                .is_some_and(|v| ('1'..='9').contains(&v))
        });
        let style_index = format.iter().position(|key| key == "style");
        let panel =
            drawing && style_index.is_some_and(|i| fields[i].trim().eq_ignore_ascii_case("Panel"));
        let effect = format
            .iter()
            .position(|key| key == "effect")
            .is_some_and(|i| !fields[i].trim().is_empty());
        result.push(Event {
            style_only,
            effect,
            line,
            fields,
            start,
            end,
            style_index,
            prefix,
            body,
            from,
            to,
            drawing,
            panel,
        });
    }
    if !styles || result.is_empty() {
        return Err("스타일과 Dialogue 행이 있는 ASS 파일을 선택해 주세요.".into());
    }
    Ok(result)
}

fn events(source: &str) -> Result<Vec<Event<'_>>, String> {
    Ok(all_events(source)?
        .into_iter()
        .filter(|e| !e.style_only)
        .collect())
}

impl AssTrack {
    pub fn validate(&self) -> Result<(), String> {
        events(&self.source).map(|_| ())
    }
}

#[tauri::command]
pub(crate) fn ass_rows(source: String) -> Result<Vec<Row>, String> {
    let mut rows: Vec<_> = events(&source)?
        .into_iter()
        .filter(|e| !e.drawing)
        .map(|e| Row {
            line: e.line,
            start_ms: e.from,
            end_ms: e.to,
            text: e.body.replace("\\N", "\n"),
        })
        .collect();
    flow_rows::editor_order(&source, &mut rows);
    Ok(rows)
}

// Edit by physical line, so overlapping layers, duplicate text and arbitrary event order retain identity.
#[tauri::command]
pub(crate) fn ass_update(
    source: String,
    edits: Vec<Row>,
    duration_ms: u64,
    additions: Option<Vec<NewRow>>,
) -> Result<String, String> {
    let (source, edits) = match additions {
        Some(rows) if !rows.is_empty() => new_rows::expand(&source, edits, rows)?,
        _ => (source, edits),
    };
    let parsed = events(&source)?;
    let mut lines: Vec<String> = source.lines().map(str::to_owned).collect();
    let mut changed = std::collections::HashMap::new();
    let mut deleted = std::collections::HashSet::new();
    for edit in &edits {
        let e = parsed
            .iter()
            .find(|e| e.line == edit.line && !e.drawing)
            .ok_or("수정할 자막을 찾을 수 없습니다.")?;
        if deleted.contains(&e.line) || changed.contains_key(&e.line) {
            return Err("같은 자막이 중복되었습니다.".into());
        }
        if edit.text.trim().is_empty() {
            deleted.insert(e.line);
            continue;
        }
        if edit.start_ms >= edit.end_ms
            || edit.end_ms > duration_ms
            || edit.start_ms % 10 != 0
            || edit.end_ms % 10 != 0
            || edit.text.contains(['\r', '\0'])
        {
            return Err("자막 문구와 구간 안의 시작·끝 시간(0.01초 단위)을 확인해 주세요.".into());
        }
        changed.insert(e.line, (edit.start_ms, edit.end_ms));
        lines[e.line] = rewrite(
            e,
            edit.start_ms,
            edit.end_ms,
            &format!("{}{}", e.prefix, edit.text.replace('\n', "\\N")),
        );
    }
    // A panel spanning exactly a group of captions follows the group's outer edges.
    for e in parsed.iter().filter(|e| e.panel) {
        let covered: Vec<_> = parsed
            .iter()
            .filter(|r| !r.drawing && r.from >= e.from && r.to <= e.to)
            .collect();
        if covered.iter().any(|r| r.from == e.from) && covered.iter().any(|r| r.to == e.to) {
            let bounds: Vec<_> = covered
                .iter()
                .map(|r| changed.get(&r.line).copied().unwrap_or((r.from, r.to)))
                .collect();
            let a = bounds.iter().map(|v| v.0).min().unwrap();
            let b = bounds.iter().map(|v| v.1).max().unwrap();
            if (a, b) != (e.from, e.to) {
                lines[e.line] = rewrite(e, a, b, e.fields.last().unwrap());
            }
        }
    }
    let sep = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut result = lines.join(sep);
    if source.ends_with('\n') {
        result.push_str(sep);
    }
    if deleted.is_empty() {
        Ok(result)
    } else {
        delete_matching(&result, |e| deleted.contains(&e.line))
    }
}

#[derive(Serialize)]
pub(crate) struct PreviewRange {
    start: u64,
    end: u64,
    animated: bool,
}

// At 30 fps a frame is exactly 1000 ticks and a millisecond is 30 ticks.
// FFmpeg truncates floating-point milliseconds before calling libass. One
// extra tick keeps an exact boundary (e.g. frame 414 at 13.80s) on its
// intended side without reaching the next millisecond on any frame.
pub(super) fn render_filter(offset_ticks: u64, transparent: bool) -> String {
    let alpha = if transparent { ":alpha=1" } else { "" };
    format!(
        "settb=1/30000,setpts=PTS+{},ass=scene.ass{alpha},setpts=PTS-STARTPTS,settb=1/30",
        offset_ticks + 1
    )
}

#[tauri::command]
pub(crate) fn ass_ranges(source: String) -> Result<Vec<PreviewRange>, String> {
    // Ranges are in output frames. Keep event boundaries even when two static captions touch.
    let mut points = std::collections::BTreeMap::<u64, (i32, i32)>::new();
    for event in events(&source)? {
        let frame = |ms: u64| (ms * 30).div_ceil(1000);
        let (start, end) = (frame(event.from), frame(event.to));
        if start >= end {
            continue;
        }
        points.entry(start).or_default().0 += 1;
        points.entry(end).or_default().0 -= 1;
        let text = event.fields.last().unwrap().to_ascii_lowercase();
        let mut motion = |a: u64, b: u64| {
            if a < b {
                points.entry(a).or_default().1 += 1;
                points.entry(b).or_default().1 -= 1;
            }
        };
        if event.effect
            || ["\\move", "\\t(", "\\fade(", "\\k"]
                .iter()
                .any(|tag| text.contains(tag))
        {
            motion(start, end);
        } else if let Some((_, tail)) = text.split_once("\\fad(") {
            let fades = tail
                .split_once(')')
                .and_then(|(args, _)| args.split_once(','))
                .and_then(|(a, b)| {
                    Some((a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?))
                });
            if let Some((a, b)) = fades {
                motion(start, frame(event.from + u64::from(a)).min(end));
                motion(frame(event.to.saturating_sub(u64::from(b))).max(start), end);
            } else {
                motion(start, end);
            }
        }
    }
    let (mut visible, mut moving, mut previous) = (0, 0, 0);
    let mut ranges = Vec::new();
    for (at, (delta, anim)) in points {
        if previous < at && visible > 0 {
            ranges.push(PreviewRange {
                start: previous,
                end: at,
                animated: moving > 0,
            });
        }
        visible += delta;
        moving += anim;
        previous = at;
    }
    Ok(ranges)
}

fn rewrite(e: &Event<'_>, from: u64, to: u64, text: &str) -> String {
    let mut values = e.fields.clone();
    let (a, b) = (stamp(from), stamp(to));
    values[e.start] = &a;
    values[e.end] = &b;
    *values.last_mut().unwrap() = text;
    format!("Dialogue: {}", values.join(","))
}

pub(super) fn insert(source: &str, at: u64, duration: u64, text: &str) -> Result<String, String> {
    let mut parsed = all_events(source)?;
    if parsed.iter().any(|e| e.style_only && !e.drawing)
        && !parsed.iter().any(|e| !e.style_only && !e.drawing)
    {
        let (from, to) = super::caption_track::insertion_range(at, duration, 10, None, None)?;
        let mut lines: Vec<_> = source.lines().map(str::to_owned).collect();
        for e in parsed.iter().filter(|e| e.style_only) {
            let body = if e.drawing {
                e.fields.last().unwrap().to_string()
            } else {
                format!("{}{}", e.prefix, text.replace('\n', "\\N"))
            };
            lines[e.line] = rewrite(e, from, to, &body);
        }
        return join_lines(source, lines);
    }
    parsed.retain(|e| !e.style_only);
    if parsed.is_empty() {
        return Err("새 자막의 스타일을 찾을 수 없습니다.".into());
    }
    let start = at / 10 * 10;
    let captions = || parsed.iter().filter(|e| !e.drawing);
    let active = captions()
        .filter(|e| e.from <= start && start < e.to)
        .max_by_key(|e| (e.from, std::cmp::Reverse(e.line)));
    let next = captions().filter(|e| e.from > start).min_by_key(|e| e.from);
    let (from, to) = super::caption_track::insertion_range(
        at,
        duration,
        10,
        active.map(|e| (e.from, e.to)),
        next.map(|e| e.from),
    )?;
    let template = active.or_else(|| {
        captions().min_by_key(|e| start.saturating_sub(e.to).max(e.from.saturating_sub(start)))
    });
    let mut lines: Vec<String> = source.lines().map(str::to_owned).collect();
    if let Some(current) = active {
        let (a, b) = if from == current.from {
            (to, current.to)
        } else {
            (current.from, from)
        };
        for row in captions().filter(|e| (e.from, e.to) == (current.from, current.to)) {
            lines[row.line] = rewrite(row, a, b, row.fields.last().unwrap());
        }
    }
    let body = text.replace('\n', "\\N");
    let template = template.unwrap_or(&parsed[0]);
    let new_line = if template.drawing {
        // No dialogue style to copy. Use the file's event format and libass's Default style.
        let mut fields = template.fields.clone();
        if let Some(i) = template.style_index {
            fields[i] = "Default";
        }
        let fallback = Event {
            fields,
            ..*template
        };
        rewrite(&fallback, from, to, &body)
    } else {
        rewrite(template, from, to, &format!("{}{body}", template.prefix))
    };
    // Place each clone next to its template, honoring changes of Format inside the file.
    let insert_after = flow_rows::groups(source)
        .find(|group| group.contains(&template.line))
        .map_or(template.line, |group| *group.end());
    let mut additions = vec![(insert_after, new_line)];
    if !parsed
        .iter()
        .any(|p| p.panel && p.from <= from && to <= p.to)
    {
        for p in parsed
            .iter()
            .filter(|p| p.panel && p.from <= template.from && template.to <= p.to)
        {
            additions.push((p.line, rewrite(p, from, to, p.fields.last().unwrap())));
        }
    }
    additions.sort_by_key(|(line, _)| std::cmp::Reverse(*line));
    for (line, value) in additions {
        lines.insert(line + 1, value);
    }
    join_lines(source, lines)
}

fn join_lines(source: &str, lines: Vec<String>) -> Result<String, String> {
    let sep = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut result = lines.join(sep);
    if source.ends_with('\n') {
        result.push_str(sep);
    }
    events(&result)?;
    Ok(result)
}

pub(super) fn delete(source: &str, start: u64, end: u64) -> Result<String, String> {
    delete_matching(source, |e| e.from == start && e.to == end)
}

fn delete_matching(source: &str, selected: impl Fn(&Event) -> bool) -> Result<String, String> {
    let parsed = events(source)?;
    let captions: Vec<_> = parsed.iter().filter(|e| !e.drawing).collect();
    let first = captions
        .iter()
        .find(|e| selected(e))
        .ok_or("삭제할 자막을 찾을 수 없습니다.")?;
    let last_group = captions.iter().all(|e| selected(e));
    let mut replacements = std::collections::HashMap::new();
    for e in parsed.iter() {
        if !e.drawing && selected(e) {
            let mut lines = Vec::new();
            if last_group && e.line == first.line {
                lines.push(rewrite(e, e.from, e.to, e.prefix).replacen(
                    "Dialogue",
                    STYLE_TEMPLATE,
                    1,
                ));
            }
            replacements.insert(e.line, lines);
        } else if e.panel
            && captions.iter().any(|r| r.from == e.from && r.to <= e.to)
            && captions.iter().any(|r| r.to == e.to && r.from >= e.from)
        {
            // Remove only the part of a shared panel no surviving caption needs.
            let mut removed: Vec<_> = captions
                .iter()
                .filter(|r| selected(r) && e.from <= r.from && r.to <= e.to)
                .map(|r| (r.from, r.to))
                .collect();
            if removed.is_empty() {
                continue;
            }
            removed.sort_unstable();
            let mut keep = Vec::new();
            let mut cursor = e.from;
            for (a, b) in removed {
                if cursor < a {
                    keep.push((cursor, a));
                }
                cursor = cursor.max(b);
            }
            keep.push((cursor, e.to));
            keep.extend(
                captions
                    .iter()
                    .filter(|r| !selected(r))
                    .map(|r| (r.from.max(e.from), r.to.min(e.to))),
            );
            keep.retain(|(a, b)| a < b);
            keep.sort_unstable();
            let mut spans: Vec<(u64, u64)> = Vec::new();
            for (a, b) in keep {
                if let Some(prev) = spans.last_mut().filter(|prev| prev.1 >= a) {
                    prev.1 = prev.1.max(b);
                } else {
                    spans.push((a, b));
                }
            }
            if spans.as_slice() == [(e.from, e.to)] {
                continue;
            }
            let mut lines: Vec<_> = spans
                .into_iter()
                .map(|(a, b)| rewrite(e, a, b, e.fields.last().unwrap()))
                .collect();
            if last_group && e.from <= first.from && first.to <= e.to {
                lines.push(
                    rewrite(e, first.from, first.to, e.fields.last().unwrap()).replacen(
                        "Dialogue",
                        STYLE_TEMPLATE,
                        1,
                    ),
                );
            }
            replacements.insert(e.line, lines);
        }
    }
    let mut lines = Vec::new();
    for (i, line) in source.lines().enumerate() {
        if let Some(replacement) = replacements.remove(&i) {
            lines.extend(replacement);
        } else {
            lines.push(line.to_owned());
        }
    }
    join_lines(source, lines)
}

use super::{Event, new_rows};

// Some guides place the two halves of one caption in individually centered
// slots. A hard break centers the entire text block around a single anchor.
pub(super) fn paired_prefix(
    source: &str,
    events: &[Event<'_>],
    template: &Event<'_>,
    text: &str,
    include_peer: impl Fn(usize) -> bool,
) -> Option<String> {
    let (first, second) = text.split_once('\n')?;
    if first.trim().is_empty() || second.trim().is_empty() || second.contains('\n') {
        return None;
    }
    let (coords, x, y) = static_slot(template)?;
    let (_, other_y) = paired_slot(source, events, template, include_peer)?;
    let mut prefix = template.prefix.to_owned();
    prefix.replace_range(coords, &format!("{x},{}", (y + other_y) / 2.0));
    Some(prefix)
}

pub(super) fn paired_slot(
    source: &str,
    events: &[Event<'_>],
    template: &Event<'_>,
    include_peer: impl Fn(usize) -> bool,
) -> Option<(usize, f64)> {
    let (_, x, y) = static_slot(template)?;
    let (size, scale, alignment) = new_rows::style_metrics(source, template);
    if !(4..=6).contains(&alignment) {
        return None;
    }
    let metadata = template.fields.split_last()?.1;
    let mut peers = events.iter().filter_map(|peer| {
        if peer.line == template.line
            || peer.fields.split_last()?.1 != metadata
            || !include_peer(peer.line)
        {
            return None;
        }
        let (_, other_x, other_y) = static_slot(peer)?;
        let distance = (other_y - y).abs();
        // Avoid unrelated labels laid out in separate columns or far apart.
        (other_x == x && distance > 0.0 && distance <= 2.0 * size * scale / 100.0)
            .then_some((peer.line, other_y))
    });
    let peer = peers.next()?;
    if peers.next().is_some() {
        return None;
    }
    Some(peer)
}

fn static_slot(e: &Event<'_>) -> Option<(std::ops::Range<usize>, f64, f64)> {
    if e.drawing || e.effect || e.body.contains(['{', '}']) || e.body.contains(r"\N") {
        return None;
    }
    // Only borrow a position from simple fixed captions. Other layouts retain
    // the template's authored position, including motion and alignment overrides.
    let tags = e
        .prefix
        .split(['{', '}', '\\'])
        .filter(|tag| !tag.is_empty());
    if !tags.into_iter().all(|tag| {
        tag == "an5"
            || ["pos(", "fad(", "fscx", "fscy"]
                .iter()
                .any(|s| tag.starts_with(s))
    }) {
        return None;
    }
    let (coords, args, y) = new_rows::position(e.prefix)?;
    let x: f64 = args.first()?.parse().ok()?;
    (args.len() == 2 && x.is_finite() && y.is_finite()).then_some((coords, x, y))
}

#[cfg(test)]
mod tests {
    use super::super::{ass_rows, ass_update, insert};

    const GUIDE: &str = r#"[Script Info]
PlayResX: 1920
PlayResY: 1080
[V4+ Styles]
Format: Name, Fontsize, ScaleY, Alignment
Style: Caption,71.5,100,5
[Events]
Format: Layer, Start, End, Style, Text
Dialogue: 1,0:00:00.00,0:00:02.00,Caption,{\fscx101.2\fscy102.13\pos(960,956)}윗줄
Dialogue: 1,0:00:00.00,0:00:02.00,Caption,{\fscx102.1\fscy100\pos(960,1016)}아랫줄
"#;

    #[test]
    fn a_merged_or_new_multiline_caption_uses_the_center_of_both_slots() {
        let mut edits = ass_rows(GUIDE.into()).unwrap();
        edits[0].text = "윗줄\n아랫줄".into();
        edits[1].text.clear();
        let merged = ass_update(GUIDE.into(), edits, 4000, None).unwrap();
        let positioned = r"{\fscx101.2\fscy102.13\pos(960,986)}윗줄\N아랫줄";
        assert!(merged.contains(positioned));
        assert_eq!(ass_rows(merged).unwrap().len(), 1);
        let inserted = insert(GUIDE, 2500, 4000, "윗줄\n아랫줄").unwrap();
        assert!(inserted.contains(positioned));
        assert_eq!(ass_rows(inserted).unwrap().len(), 3);
    }

    #[test]
    fn editing_alongside_another_caption_or_reusing_unrelated_positions_keeps_the_anchor() {
        let mut edits = ass_rows(GUIDE.into()).unwrap();
        edits.truncate(1);
        edits[0].text = "첫째\n둘째".into();
        let original_slot = r"\pos(960,956)}첫째\N둘째";
        assert!(
            ass_update(GUIDE.into(), edits, 4000, None)
                .unwrap()
                .contains(original_slot)
        );
        for other in ["960,300", "620,1016"] {
            let source = GUIDE.replace("960,1016", other);
            assert!(
                insert(&source, 2500, 4000, "첫째\n둘째")
                    .unwrap()
                    .contains(original_slot)
            );
        }
    }
}

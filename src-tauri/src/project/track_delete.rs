use super::{
    model::{Asset, Kind, MIN_DURATION_MS, Overlay, Project, SceneClip, SceneSettings},
    undo::Inverse,
};
use std::ops::Range;
use uuid::Uuid;

struct Piece {
    index: usize,
    start: u64,
    keep: Range<u64>,
    second_half: bool,
}

// The inputs are non-overlapping placements on one track, in scene time order.
// Only the continuous run touching the exclusive end follows the deleted range.
fn survivors(
    spans: impl Iterator<Item = (u64, u64)>,
    cut: &Range<u64>,
) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut touched = false;
    let mut ripple_end = cut.end;
    for (index, (start, duration)) in spans.enumerate() {
        let end = start + duration;
        if end <= cut.start || start >= cut.end {
            let position = if start == ripple_end {
                ripple_end = end;
                start - (cut.end - cut.start)
            } else {
                start
            };
            pieces.push(Piece {
                index,
                start: position,
                keep: 0..duration,
                second_half: false,
            });
            continue;
        }
        touched = true;
        if start < cut.start {
            pieces.push(Piece {
                index,
                start,
                keep: 0..cut.start - start,
                second_half: false,
            });
        }
        if cut.end < end {
            pieces.push(Piece {
                index,
                start: cut.start,
                keep: cut.end - start..duration,
                second_half: start < cut.start,
            });
            ripple_end = end;
        }
    }
    if !touched {
        return Err("선택한 트랙의 범위에 삭제할 클립이 없습니다.".into());
    }
    Ok(pieces)
}

fn check_length(asset: &Asset, length: u64) -> Result<(), String> {
    let minimum = asset
        .duration_ms
        .unwrap_or(MIN_DURATION_MS)
        .clamp(1, MIN_DURATION_MS);
    if length < minimum {
        return Err(
            "클립을 전부 삭제하거나 남는 조각이 0.1초 이상이 되도록 범위를 조정해 주세요.".into(),
        );
    }
    Ok(())
}

fn slice_main(
    project: &Project,
    mut clip: SceneClip,
    keep: &Range<u64>,
) -> Result<SceneClip, String> {
    let asset = project.asset(clip.visual.source.asset_id)?;
    let old_duration = clip.duration_ms;
    clip.duration_ms = keep.end - keep.start;
    check_length(asset, clip.duration_ms)?;
    if asset.kind == Kind::Image {
        return Ok(clip);
    }
    let source = &mut clip.visual.source;
    if let Some(trim) = &mut clip.visual.trim {
        let visible_start = trim.offset_ms;
        let visible_end = visible_start.saturating_add(trim.out_ms - source.in_ms);
        let first = visible_start.max(keep.start);
        let last = visible_end.min(keep.end);
        if first < last {
            if keep.end < old_duration {
                trim.out_ms = source.in_ms + (last - visible_start);
            }
            source.in_ms += first - visible_start;
            trim.offset_ms = first - keep.start;
        } else {
            // Keep an all-padding remainder blank, with valid source bounds.
            trim.offset_ms = clip.duration_ms;
        }
    } else {
        source.in_ms = source
            .in_ms
            .checked_add(keep.start)
            .ok_or("원본 위치를 확인해 주세요.")?;
        if asset.duration_ms.is_some_and(|ms| source.in_ms >= ms) {
            return Err("원본 영상이 끝난 뒤의 정지 화면만 남습니다. 삭제 끝점을 원본 안이나 클립 끝에 놓아 주세요.".into());
        }
    }
    Ok(clip)
}

fn delete_main(
    project: &Project,
    settings: &mut SceneSettings,
    scene_id: Uuid,
    cut: &Range<u64>,
) -> Result<(), String> {
    let mut clips = std::mem::take(&mut settings.clips);
    if clips.is_empty()
        && let Some(visual) = settings.visual.take()
    {
        clips.push(SceneClip {
            id: scene_id,
            visual,
            gap_ms: 0,
            duration_ms: settings.duration_ms,
        });
    }
    let spans = clips.iter().scan(0, |end, clip| {
        let start = *end + clip.gap_ms;
        *end = start + clip.duration_ms;
        Some((start, clip.duration_ms))
    });
    let mut pieces = survivors(spans, cut)?.into_iter().peekable();
    let mut end = 0;
    while let Some(mut piece) = pieces.next() {
        let original = clips[piece.index];
        if let Some(right) = pieces.peek().filter(|p| p.index == piece.index) {
            let asset = project.asset(original.visual.source.asset_id)?;
            // An untrimmed legacy clip continues to show its final frame after EOF.
            // Removing time wholly within that still tail only shortens the same clip.
            if original.visual.trim.is_none()
                && asset.kind == Kind::Video
                && asset.duration_ms.is_some_and(|ms| {
                    piece.keep.end >= ms.saturating_sub(original.visual.source.in_ms)
                })
            {
                piece.keep.end += right.keep.end - right.keep.start;
                pieces.next();
            }
        }
        let mut clip = if piece.keep == (0..original.duration_ms) {
            original
        } else {
            slice_main(project, original, &piece.keep)?
        };
        if piece.second_half {
            clip.id = Uuid::new_v4();
        }
        clip.gap_ms = piece.start - end;
        end = piece.start + clip.duration_ms;
        settings.clips.push(clip);
    }
    Ok(())
}

fn delete_overlays(
    project: &Project,
    settings: &mut SceneSettings,
    track: u32,
    cut: &Range<u64>,
) -> Result<(), String> {
    let mut target = Vec::new();
    let mut others = Vec::new();
    for (i, mut layer) in settings.overlays.drain(..).enumerate() {
        // Pin legacy track numbers before intervals disappear or split.
        if *layer.track.get_or_insert(i as u32 + 1) == track {
            target.push(layer);
        } else {
            others.push(layer);
        }
    }
    target.sort_by_key(|layer| layer.start_ms);
    let pieces = survivors(target.iter().map(|o| (o.start_ms, o.duration_ms)), cut)?;
    for piece in pieces {
        let mut layer: Overlay = target[piece.index];
        layer.start_ms = piece.start;
        if piece.keep != (0..layer.duration_ms) {
            let asset = project.asset(layer.asset_id)?;
            layer.duration_ms = piece.keep.end - piece.keep.start;
            check_length(asset, layer.duration_ms)?;
            if asset.kind == Kind::Video {
                layer.in_ms = layer
                    .in_ms
                    .checked_add(piece.keep.start)
                    .ok_or("원본 위치를 확인해 주세요.")?;
            }
        }
        if piece.second_half {
            layer.id = Uuid::new_v4();
        }
        others.push(layer);
    }
    settings.overlays = others;
    Ok(())
}

pub(super) fn apply(
    project: &mut Project,
    scene_id: Uuid,
    expected: u32,
    track: u32,
    start_ms: u64,
    end_ms: u64,
) -> Result<Inverse, String> {
    let index = project.scene_index(scene_id)?;
    let scene = &project.scenes[index];
    if scene.revision != expected {
        return Err("구간이 변경되었습니다. 최신 내용을 확인해 주세요.".into());
    }
    if start_ms >= end_ms || end_ms > scene.settings.duration_ms {
        return Err("같은 구간 안에서 삭제 시작점과 끝점을 차례로 지정해 주세요.".into());
    }
    let before = scene.settings.clone();
    let mut settings = before.clone();
    let cut = start_ms..end_ms;
    if track == 0 {
        delete_main(project, &mut settings, scene_id, &cut)?;
    } else {
        delete_overlays(project, &mut settings, track, &cut)?;
    }
    if settings.visual.is_none()
        && settings.clips.is_empty()
        && let Some(end) = settings
            .overlays
            .iter()
            .map(|c| c.start_ms + c.duration_ms)
            .max()
    {
        settings.duration_ms = end.max(MIN_DURATION_MS);
    }
    project.edit_scene(scene_id, expected, settings)?;
    Ok(Inverse::Settings {
        id: scene_id,
        before,
        after: project.scenes[index].settings.clone(),
    })
}

#[cfg(test)]
mod padding_tests;
#[cfg(test)]
mod tests;

use super::{
    model::{Asset, Kind, MIN_DURATION_MS, Project, SceneClip, VideoTrim},
    undo::Inverse,
};
use uuid::Uuid;

fn source_time(
    asset: &Asset,
    duration_ms: u64,
    in_ms: u64,
    trim: Option<VideoTrim>,
    at_ms: u64,
) -> Result<u64, String> {
    if asset.kind == Kind::Audio
        || at_ms < MIN_DURATION_MS
        || duration_ms.saturating_sub(at_ms) < MIN_DURATION_MS
    {
        return Err("클립 앞뒤에 각각 0.1초 이상 남도록 분할 위치를 지정해 주세요.".into());
    }
    if asset.kind == Kind::Image {
        return Ok(0);
    }
    let offset = trim.map_or(0, |t| t.offset_ms);
    let cut = at_ms
        .checked_sub(offset)
        .and_then(|elapsed| in_ms.checked_add(elapsed));
    cut.filter(|&ms| {
        ms > in_ms
            && trim.is_none_or(|t| ms < t.out_ms)
            && asset.duration_ms.is_none_or(|end| ms < end)
    })
    .ok_or_else(|| "영상이 있는 위치에서 분할해 주세요.".into())
}

pub(super) fn split_main(
    project: &Project,
    clips: &mut Vec<SceneClip>,
    index: usize,
    at_ms: u64,
) -> Result<(), String> {
    let mut left = clips[index];
    let source = left.visual.source;
    let cut = source_time(
        project.asset(source.asset_id)?,
        left.duration_ms,
        source.in_ms,
        left.visual.trim,
        at_ms,
    )?;
    let mut right = left;
    right.id = Uuid::new_v4();
    right.gap_ms = 0;
    right.duration_ms -= at_ms;
    right.visual.source.in_ms = cut;
    if let Some(trim) = &mut right.visual.trim {
        trim.offset_ms = 0;
    }
    left.duration_ms = at_ms;
    if let Some(trim) = &mut left.visual.trim {
        trim.out_ms = cut;
    }
    clips.splice(index..=index, [left, right]);
    Ok(())
}

pub(super) fn apply(
    project: &mut Project,
    scene_id: Uuid,
    expected: u32,
    clip_id: Uuid,
    at_ms: u64,
) -> Result<Inverse, String> {
    let index = project.scene_index(scene_id)?;
    if project.scenes[index].revision != expected {
        return Err("구간이 변경되었습니다. 최신 내용을 확인해 주세요.".into());
    }
    let before = project.scenes[index].settings.clone();
    let mut settings = before.clone();
    if scene_id == clip_id
        && settings.clips.is_empty()
        && let Some(visual) = settings.visual.take()
    {
        settings.clips.push(SceneClip {
            id: scene_id,
            gap_ms: 0,
            duration_ms: settings.duration_ms,
            visual,
        });
    }
    if let Some(pos) = settings.clips.iter().position(|c| c.id == clip_id) {
        split_main(project, &mut settings.clips, pos, at_ms)?;
    } else {
        let pos = settings
            .overlays
            .iter()
            .position(|c| c.id == clip_id)
            .ok_or("영상 클립을 찾을 수 없습니다.")?;
        // Legacy overlays derive track numbers from their position. Fix those
        // numbers before inserting the second piece, so it stays on the same track.
        for (i, overlay) in settings.overlays.iter_mut().enumerate() {
            overlay.track.get_or_insert(i as u32 + 1);
        }
        let mut left = settings.overlays[pos];
        let cut = source_time(
            project.asset(left.asset_id)?,
            left.duration_ms,
            left.in_ms,
            None,
            at_ms,
        )?;
        let mut right = left;
        right.id = Uuid::new_v4();
        right.start_ms += at_ms;
        right.duration_ms -= at_ms;
        right.in_ms = cut;
        left.duration_ms = at_ms;
        settings.overlays.splice(pos..=pos, [left, right]);
    }
    project.edit_scene(scene_id, expected, settings)?;
    Ok(Inverse::Settings {
        id: scene_id,
        before,
        after: project.scenes[index].settings.clone(),
    })
}

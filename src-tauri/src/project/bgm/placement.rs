use super::{Anchor, Clip, Kind, Project};

impl Clip {
    fn source_length(&self, duration: u64) -> u64 {
        self.source_range
            .map_or(duration, |r| r.out_ms.min(duration).saturating_sub(r.in_ms))
    }

    // Scene-end anchors follow later edits, up to one pass through this song or excerpt.
    pub(in crate::project) fn playback_end(
        &self,
        start: u64,
        anchor_end: u64,
        asset_duration: Option<u64>,
    ) -> u64 {
        match self.source_duration_ms {
            None => anchor_end,
            Some(recorded) => {
                let duration = asset_duration.filter(|&d| d > 0).unwrap_or(recorded);
                anchor_end.min(start.saturating_add(self.source_length(duration)))
            }
        }
    }
}

impl Project {
    fn bgm_anchor_at(&self, time: u64, is_end: bool) -> Result<Anchor, String> {
        let mut elapsed = 0u64;
        for scene in &self.scenes {
            let next = elapsed.saturating_add(scene.settings.duration_ms);
            if time < next || (is_end && time == next) {
                return Ok(Anchor {
                    scene_id: scene.id,
                    offset_ms: Some(time - elapsed),
                });
            }
            elapsed = next;
        }
        Err("BGM 지점이 타임라인 길이를 벗어납니다.".into())
    }

    pub(super) fn normalize_bgm_clip(
        &self,
        clip: &mut Clip,
        prior: Option<&Clip>,
    ) -> Result<(), String> {
        let source = self
            .assets
            .iter()
            .find(|a| a.id == clip.asset_id && a.kind == Kind::Audio)
            .ok_or("BGM 오디오 소스를 찾을 수 없습니다.")?;
        if prior.is_none() && (source.removed_from_bin || source.missing) {
            return Err("보관함에서 사용할 수 있는 음악을 선택해 주세요.".into());
        }
        if clip.source_duration_ms == Some(0) {
            return Err("원본 음악 길이는 0보다 커야 합니다.".into());
        }
        let previous_duration = prior.and_then(|old| old.source_duration_ms);
        let duration = source
            .duration_ms
            .filter(|&d| d > 0)
            .or(previous_duration)
            .or(clip.source_duration_ms);
        let same_source_range = prior.is_some_and(|old| old.source_range == clip.source_range);
        if let Some(r) = clip.source_range
            && (r.in_ms >= r.out_ms
                || (!same_source_range && duration.is_none_or(|d| r.out_ms > d)))
        {
            return Err("곡 사용 범위는 원본 음악 길이 안에 있어야 합니다.".into());
        }
        for anchor in [clip.start, clip.end] {
            let scene = &self.scenes[self.scene_index(anchor.scene_id)?];
            if anchor
                .offset_ms
                .is_some_and(|ms| ms > scene.settings.duration_ms)
                && !prior.is_some_and(|old| anchor == old.start || anchor == old.end)
            {
                return Err("BGM 지점이 지정한 구간 길이를 벗어납니다.".into());
            }
        }
        let start = self
            .anchor_time(clip.start)
            .ok_or("BGM 시작 구간을 찾을 수 없습니다.")?;
        let mut end = self
            .anchor_time(clip.end)
            .ok_or("BGM 끝 구간을 찾을 수 없습니다.")?;
        if start >= end || clip.volume > 100 {
            return Err("BGM 범위와 음량을 확인해 주세요.".into());
        }

        // Preserve authored looping, and scene-relative endpoints, for pure moves or mix edits.
        let same_length = prior.is_some_and(|old| {
            self.anchor_time(old.end)
                .zip(self.anchor_time(old.start))
                .and_then(|(end, start)| end.checked_sub(start))
                == Some(end - start)
        });
        if same_source_range
            && same_length
            && (previous_duration.is_some() || clip.source_duration_ms.is_none())
        {
            clip.source_duration_ms = previous_duration;
            return Ok(());
        }

        let duration = duration.ok_or("원본 음악 길이를 확인한 후 BGM 범위를 조절해 주세요.")?;
        let max_length = clip.source_length(duration);
        if max_length == 0 {
            return Err("곡 사용 범위가 원본 음악 길이를 벗어납니다.".into());
        }
        clip.source_duration_ms = Some(duration);
        let keep_end = prior.is_some_and(|old| old.end == clip.end);
        if let Some(old) = prior.filter(|_| keep_end)
            && let Some(old_start) = self.anchor_time(old.start)
        {
            // Editing only the left edge or the excerpt keeps the visible right edge fixed.
            let visible_end = old.playback_end(old_start, end, source.duration_ms);
            if visible_end < end {
                if start >= visible_end {
                    return Err("BGM 시작 지점은 재생 끝 지점보다 앞에 있어야 합니다.".into());
                }
                end = visible_end;
                clip.end = self.bgm_anchor_at(end, true)?;
            }
        }
        if end - start > max_length {
            if same_source_range && keep_end {
                let bounded = end - max_length;
                if let Some(old) = prior.filter(|old| self.anchor_time(old.start) == Some(bounded))
                {
                    clip.start = old.start;
                    clip.end = old.end;
                } else {
                    clip.start = self.bgm_anchor_at(bounded, false)?;
                }
            } else {
                let bounded = start + max_length;
                let unchanged = prior
                    .filter(|old| old.start == clip.start && same_source_range)
                    .filter(|old| {
                        self.anchor_time(old.end).is_some_and(|e| {
                            old.playback_end(start, e, source.duration_ms) == bounded
                        })
                    });
                clip.end = match unchanged {
                    Some(old) => old.end,
                    None => self.bgm_anchor_at(bounded, true)?,
                };
            }
        }
        Ok(())
    }
}

use super::composition_layout::CaptionChange;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CaptionCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

// Caption text uses LF in the editor and project model, including text pasted from Windows.
pub(super) fn normalize_line_endings(text: &mut String) {
    if text.contains('\r') {
        *text = text.replace("\r\n", "\n").replace('\r', "\n");
    }
}

// Both editors insert at the playhead. At a shared boundary the new cue plays
// first, leaving at least one tick for the original text.
pub(super) fn insertion_range(
    at: u64,
    duration: u64,
    tick: u64,
    current: Option<(u64, u64)>,
    next: Option<u64>,
) -> Result<(u64, u64), String> {
    let start = at / tick * tick;
    let limit = duration / tick * tick;
    let end = match current {
        Some((a, b)) if start == a => {
            let half = (b.min(limit).saturating_sub(a) / tick / 2).min(2000 / tick);
            a + half * tick
        }
        Some((_, b)) => b.min(limit),
        None => start
            .saturating_add(2000)
            .min(next.unwrap_or(limit))
            .min(limit),
    };
    if start >= end {
        return Err("이 위치에는 새 자막을 넣을 시간이 없습니다. 재생 위치를 옮겨 주세요.".into());
    }
    Ok((start, end))
}

pub(super) fn insert(
    cues: &mut Vec<CaptionCue>,
    at: u64,
    duration: u64,
    text: String,
) -> Result<(), String> {
    let i = cues.partition_point(|c| c.end_ms <= at);
    let current = cues
        .get(i)
        .filter(|c| c.start_ms <= at)
        .map(|c| (c.start_ms, c.end_ms));
    let (start_ms, end_ms) =
        insertion_range(at, duration, 1, current, cues.get(i).map(|c| c.start_ms))?;
    let mut index = i;
    if let Some((start, _)) = current {
        if start == at {
            cues[i].start_ms = end_ms;
        } else {
            cues[i].end_ms = at;
            index += 1;
        }
    }
    cues.insert(
        index,
        CaptionCue {
            start_ms,
            end_ms,
            text,
        },
    );
    validate(cues, duration)
}

pub(super) fn validate(cues: &[CaptionCue], duration: u64) -> Result<(), String> {
    let mut previous_end = 0;
    for cue in cues {
        if cue.start_ms < previous_end || cue.start_ms >= cue.end_ms || cue.end_ms > duration {
            return Err("자막의 시작과 끝을 구간 안에서 겹치지 않게 지정해 주세요.".into());
        }
        if cue.text.trim().is_empty() || cue.text.contains('\0') {
            return Err("표시할 자막 문구를 입력해 주세요.".into());
        }
        previous_end = cue.end_ms;
    }
    Ok(())
}

// Project time is relative to the script section. Each exported clip has its own clock.
pub(super) fn window(cues: &[CaptionCue], start: u64, duration: u64) -> Vec<CaptionCue> {
    let end = start.saturating_add(duration);
    cues.iter()
        .filter(|cue| cue.start_ms < end && cue.end_ms > start)
        .map(|cue| CaptionCue {
            start_ms: cue.start_ms.saturating_sub(start),
            end_ms: cue.end_ms.min(end) - start,
            text: cue.text.clone(),
        })
        .collect()
}

pub(super) fn changes(cues: &[CaptionCue]) -> Vec<CaptionChange> {
    let mut changes = Vec::new();
    for (i, cue) in cues.iter().enumerate() {
        changes.push(CaptionChange {
            start_ms: cue.start_ms,
            text: cue.text.clone(),
        });
        if cues
            .get(i + 1)
            .is_none_or(|next| next.start_ms > cue.end_ms)
        {
            changes.push(CaptionChange {
                start_ms: cue.end_ms,
                text: String::new(),
            });
        }
    }
    changes
}

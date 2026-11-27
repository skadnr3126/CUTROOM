import type { BgmAnchor, BgmClip } from "./project";
import type { timelineSections } from "./timeline";

export type Sections = ReturnType<typeof timelineSections>;
export function anchorTime(anchor: BgmAnchor, sections: Sections) {
  const section = sections.find(s => s.scene.id === anchor.sceneId);
  if (!section) return 0;
  return section.start + Math.min(anchor.offsetMs ?? section.durationMs, section.durationMs);
}

function sourceSpan(clip: BgmClip, duration: number) {
  if (clip.sourceRange) {
    const { inMs, outMs } = clip.sourceRange;
    return Math.max(0, Math.min(duration, outMs) - inMs);
  }
  return duration;
}

export function bgmSourceDuration(clip: BgmClip, durationMs?: number | null) {
  return durationMs && durationMs > 0 ? durationMs : clip.sourceDurationMs;
}

export function bgmBounds(clip: BgmClip, sections: Sections, durationMs?: number | null) {
  const start = anchorTime(clip.start, sections);
  const end = anchorTime(clip.end, sections);
  // Clips saved before source-duration limits could deliberately loop.
  if (!clip.sourceDurationMs) return { start, end };
  const duration = bgmSourceDuration(clip, durationMs) ?? clip.sourceDurationMs;
  return { start, end: Math.min(end, start + sourceSpan(clip, duration)) };
}

// At a shared boundary, starts belong to the following scene and ends to the preceding scene.
export function anchorAt(sections: Sections, time: number, edge: "start" | "end", snapMs: number, followSceneEnd = true): BgmAnchor {
  const last = sections[sections.length - 1];
  const clamped = Math.min(last.end, Math.max(0, Math.round(time)));
  const nearest = sections.reduce((best, s) => Math.abs(s.end-clamped) < Math.abs(best-clamped) ? s.end : best, 0);
  const at = Math.abs(nearest-clamped) <= snapMs ? nearest : clamped;
  const section = sections.find(s => edge === "end" ? s.end >= at : s.end > at) ?? last;
  return {sceneId:section.scene.id, offsetMs:followSceneEnd && at === section.end ? null : at-section.start};
}

export function moveBgm(clip: BgmClip, sections: Sections, startMs: number, snapMs: number, durationMs?: number | null): BgmClip {
  const { start: original, end } = bgmBounds(clip, sections, durationMs);
  const length = end - original;
  const lastStart = sections[sections.length - 1].end - length;
  const start = Math.max(0, Math.min(lastStart, Math.round(startMs)));
  if (length <= 0 || start === original) return clip;
  // Snap either edge by translating the whole clip, so its length stays exact.
  let adjustment = 0;
  let nearest = snapMs;
  for (const boundary of [0, ...sections.map(s => s.end)]) {
    for (const delta of [boundary - start, boundary - (start + length)]) {
      if (Math.abs(delta) <= nearest && start + delta >= 0 && start + delta <= lastStart) {
        adjustment = delta;
        nearest = Math.abs(delta);
      }
    }
  }
  const moved = start + adjustment;
  if (moved === original) return clip;
  return {...clip, start:anchorAt(sections, moved, "start", 0), end:anchorAt(sections, moved + length, "end", 0)};
}

export function resizeBgm(clip: BgmClip, sections: Sections, edge: "start" | "end", requested: BgmAnchor, durationMs?: number | null): BgmClip {
  const { start, end } = bgmBounds(clip, sections, durationMs);
  const at = anchorTime(requested, sections);
  if (edge === "start" ? at >= end : at <= start) return clip;
  const sourceDuration = bgmSourceDuration(clip, durationMs);
  if (!sourceDuration) return clip;
  const maxLength = sourceSpan(clip, sourceDuration);
  if (maxLength <= 0) return clip;
  const bounded = edge === "start" ? Math.max(at, end - maxLength) : Math.min(at, start + maxLength);
  const current = edge === "start" ? start : end;
  if (at === current) return clip;
  if (bounded === current) {
    return clip.sourceDurationMs ? clip : { ...clip, sourceDurationMs: sourceDuration };
  }
  // Clamp after snapping: a nearby scene boundary can lie outside the song.
  const anchor = bounded === at ? requested : anchorAt(sections, bounded, edge, 0, false);
  const next = { ...clip, [edge]: anchor, sourceDurationMs: sourceDuration };
  if (edge === "start" && end !== anchorTime(clip.end, sections)) {
    next.end = anchorAt(sections, end, "end", 0, false);
  }
  return next;
}

export function changeBgmSourceRange(clip: BgmClip, sourceRange: BgmClip["sourceRange"], sections: Sections, durationMs: number): BgmClip {
  const { start, end } = bgmBounds(clip, sections, durationMs);
  const next = { ...clip, sourceRange, sourceDurationMs: durationMs };
  const bounded = Math.min(end, start + sourceSpan(next, durationMs));
  if (bounded !== anchorTime(clip.end, sections)) next.end = anchorAt(sections, bounded, "end", 0, false);
  return next;
}

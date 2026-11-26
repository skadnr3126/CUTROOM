import type { BgmAnchor, BgmClip } from "./project";
import type { timelineSections } from "./timeline";

export type Sections = ReturnType<typeof timelineSections>;
export function anchorTime(anchor: BgmAnchor, sections: Sections) {
  const section = sections.find(s => s.scene.id === anchor.sceneId);
  if (!section) return 0;
  return section.start + Math.min(anchor.offsetMs ?? section.durationMs, section.durationMs);
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

export function moveBgm(clip: BgmClip, sections: Sections, startMs: number, snapMs: number): BgmClip {
  const original = anchorTime(clip.start, sections);
  const length = anchorTime(clip.end, sections) - original;
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

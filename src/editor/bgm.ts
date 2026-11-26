import type { BgmAnchor } from "./project";
import type { timelineSections } from "./timeline";

export type Sections = ReturnType<typeof timelineSections>;
export function anchorTime(anchor: BgmAnchor, sections: Sections) {
  const section = sections.find(s => s.scene.id === anchor.sceneId);
  if (!section) return 0;
  return section.start + Math.min(anchor.offsetMs ?? section.durationMs, section.durationMs);
}

// At a shared boundary, starts belong to the following scene and ends to the preceding scene.
export function anchorAt(sections: Sections, time: number, edge: "start" | "end", snapMs: number): BgmAnchor {
  const last = sections[sections.length - 1];
  const clamped = Math.min(last.end, Math.max(0, Math.round(time)));
  const nearest = sections.reduce((best, s) => Math.abs(s.end-clamped) < Math.abs(best-clamped) ? s.end : best, 0);
  const at = Math.abs(nearest-clamped) <= snapMs ? nearest : clamped;
  const section = sections.find(s => edge === "end" ? s.end >= at : s.end > at) ?? last;
  return {sceneId:section.scene.id, offsetMs:at === section.end ? null : at-section.start};
}

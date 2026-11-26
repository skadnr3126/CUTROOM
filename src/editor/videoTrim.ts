import type { VisualPlacement } from "./project";

export type VideoRange = { inMs: number; outMs: number; offsetMs: number };
export type TrimEdge = "start" | "end";

export function videoCutPoint(range: VideoRange, durationMs: number, elapsedMs: number) {
  // Each saved clip must satisfy the project's minimum duration of 100 ms.
  if (elapsedMs < 100 || durationMs - elapsedMs < 100) return null;
  const sourceMs = range.inMs + elapsedMs - range.offsetMs;
  return sourceMs > range.inMs && sourceMs < range.outMs ? sourceMs : null;
}

export function videoRange(clip: VisualPlacement, sceneMs: number, sourceMs?: number | null): VideoRange {
  const offsetMs = Math.min(clip.trim?.offsetMs ?? 0, sceneMs);
  const outMs = Math.min(
    clip.trim?.outMs ?? Infinity,
    sourceMs ?? Infinity,
    clip.inMs + sceneMs - offsetMs,
  );
  return { inMs: clip.inMs, outMs: Math.max(clip.inMs, outMs), offsetMs };
}

export function sourceTime(ms: number) {
  return `${(ms / 1000).toFixed(2)}초`;
}

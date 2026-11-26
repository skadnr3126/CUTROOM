import { sceneClips, sceneSettings, type Scene, type SceneClip, type SceneSettings } from "./project";
import { videoCutPoint, videoRange, type TrimEdge } from "./videoTrim";

export type TrackClip = SceneClip & { track: number; startMs: number };
export const fullFrame = { size: 100, x: 50, y: 50 };
export function frameStyle(frame = fullFrame) {
  return {
    width: frame.size + "%", height: frame.size + "%",
    left: (100 - frame.size) * frame.x / 100 + "%", top: (100 - frame.size) * frame.y / 100 + "%",
  };
}
const maxTime = 86_400_000;
export const endOf = (clip: TrackClip) => clip.startMs + clip.durationMs;

export function visualClips(scene: Scene): TrackClip[] {
  let cursor = 0;
  const result = sceneClips(scene).map(clip => {
    const startMs = cursor + (clip.gapMs ?? 0);
    cursor = startMs + clip.durationMs;
    return { ...clip, track: 0, startMs };
  });
  for (const [i, layer] of (scene.overlays ?? []).entries()) {
    result.push({      
id: layer.id, track: layer.track ?? i + 1, startMs: layer.startMs,
      assetId: layer.assetId, durationMs: layer.durationMs, inMs: layer.inMs,
      muted: !layer.sound, frame: { size: layer.size, x: layer.x, y: layer.y }    
});
  }
  return result;
}

export function trackNumbers(clips: TrackClip[]) {
  return [...new Set([0, ...clips.map(c => c.track)])].sort((a, b) => b - a);
}

export function trackSettings(scene: Scene, items: TrackClip[]): SceneSettings {
  const settings = sceneSettings(scene);
  const sorted = [...items].sort((a, b) => a.track - b.track || a.startMs - b.startMs);
  let cursor = 0;
  const clips: SceneClip[] = [];
  const overlays = [];
  for (const item of sorted) {
    const { startMs, track, gapMs: _gap, ...clip } = item;
    if (track === 0) {
      const gap = startMs - cursor;
      clips.push(gap ? { ...clip, gapMs: gap } : clip);
      cursor = endOf(item);
    } else {
      overlays.push({        
id: clip.id, assetId: clip.assetId, startMs, track, sound: !clip.muted,
        inMs: clip.inMs, durationMs: clip.durationMs, ...(clip.frame ?? fullFrame)      
});
    }
  }
  return {    
...settings, visual: null, clips, overlays,
    durationMs: items.length ? Math.max(...items.map(endOf)) : settings.durationMs  
};
}

function inOrder(items: TrackClip[], track: number) {
  return items.filter(c => c.track === track).sort((a, b) => a.startMs - b.startMs);
}

// Move a contiguous run together. A gap absorbs the shift before later clips move.
function shiftFollowing(items: TrackClip[], original: TrackClip, delta: number) {
  let boundary = endOf(original);
  let shift = delta;
  const moved = new Map<string, number>();
  for (const clip of inOrder(items, original.track)) {
    if (clip.id === original.id || clip.startMs < boundary) continue;
    const gap = clip.startMs - boundary;
    if (gap && shift < 0) break;
    if (gap) shift = Math.max(0, shift - gap);
    if (!shift) break;
    moved.set(clip.id, clip.startMs + shift);
    boundary = endOf(clip);
  }
  return items.map(c => moved.has(c.id) ? { ...c, startMs: moved.get(c.id)! } : c);
}

export function removeClip(items: TrackClip[], clip: TrackClip) {
  return shiftFollowing(items, clip, -clip.durationMs).filter(c => c.id !== clip.id);
}

export function placeClip(items: TrackClip[], clip: TrackClip, track: number, at: number, targetId?: string, after = false) {
  if (targetId === clip.id) {
    const previousEnd = inOrder(items, track).filter(c => endOf(c) <= clip.startMs).reduce((n, c) => Math.max(n, endOf(c)), 0);
    const startMs = Math.max(previousEnd, Math.round(at));
    const shifted = shiftFollowing(items, clip, Math.max(0, startMs - clip.startMs));
    const next = shifted.map(c => c.id === clip.id ? { ...c, startMs } : c);
    return next.some(c => endOf(c) > maxTime) ? items : next;
  }
  const old = items.find(c => c.id === clip.id);
  const rest = old ? removeClip(items, old) : items;
  const target = rest.find(c => c.id === targetId);
  const startMs = target ? (after ? endOf(target) : target.startMs) : Math.max(0, Math.round(at));
  const placed = { ...clip, track, startMs };
  let cursor = endOf(placed);
  const moved = new Map<string, TrackClip>();
  for (const next of inOrder(rest, track)) {
    if (endOf(next) <= startMs) continue;
    if (next.startMs >= cursor) break;
    moved.set(next.id, { ...next, startMs: cursor });
    cursor += next.durationMs;
  }
  return cursor > maxTime ? items : [...rest.map(c => moved.get(c.id) ?? c), placed];
}

export function resizeClip(items: TrackClip[], clip: TrackClip, edge: TrimEdge, delta: number, video: boolean, sourceMs?: number | null) {
  const range = videoRange(clip, clip.durationMs, sourceMs);
  const minimum = Math.min(100, clip.durationMs);
  const previous = inOrder(items, clip.track).reverse().find(c => endOf(c) <= clip.startMs);
  const gap = clip.startMs - (previous ? endOf(previous) : 0);
  const length = video ? range.outMs - range.inMs : clip.durationMs;
  const sourceLimit = video ? (sourceMs ?? range.outMs) - range.inMs : maxTime;
  const change = edge === "start" ? Math.min(length - minimum, Math.max(delta, video ? -range.inMs : -maxTime))
    : Math.min(sourceLimit - length, Math.max(delta, minimum - length));
  const durationMs = length + (edge === "start" ? -change : change);
  const startMs = edge === "start" && gap > 0 ? clip.startMs + Math.max(-gap, change) : clip.startMs;
  const inMs = video && edge === "start" ? range.inMs + change : clip.inMs;
  const next = { ...clip, startMs, inMs, durationMs, trim: video ? { offsetMs: 0, outMs: inMs + durationMs } : undefined };
  const resized = shiftFollowing(items, clip, endOf(next) - endOf(clip)).map(c => c.id === clip.id ? next : c);
  return resized.some(c => endOf(c) > maxTime) ? items : resized;
}

export function canSplit(clip: TrackClip, at: number, video: boolean, sourceMs?: number | null) {
  const elapsed = at - clip.startMs;
  return video ? videoCutPoint(videoRange(clip, clip.durationMs, sourceMs), clip.durationMs, elapsed) !== null
    : elapsed >= 100 && clip.durationMs - elapsed >= 100;
}

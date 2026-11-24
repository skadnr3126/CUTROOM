import type { CaptionCue, Scene } from "./project";

export function sceneCaptions(scene: Scene): CaptionCue[] {
  if (scene.captionCues?.length) return scene.captionCues;
  return scene.caption.trim() ? [{ startMs: 0, endMs: scene.durationMs, text: scene.caption }] : [];
}

export function captionAt(cues: CaptionCue[], ms: number) {
  return cues.find(cue => cue.startMs <= ms && ms < cue.endMs);
}

// At an existing boundary, open that caption. Inside a caption, keep its first part.
export function insertCaption(cues: CaptionCue[], at: number, duration: number) {
  const ms = Math.min(Math.max(0, Math.round(at)), duration - 1);
  const existing = cues.findIndex(c => c.startMs === ms);
  if (existing >= 0) return { cues, index: existing };
  const next = cues.findIndex(c => c.startMs > ms);
  const index = next === -1 ? cues.length : next;
  const before = cues[index - 1];
  const list = cues.slice();
  let endMs = Math.min(duration, ms + 2000, cues[index]?.startMs ?? duration);
  if (before && before.endMs > ms) {
    endMs = before.endMs;
    list[index - 1] = { ...before, endMs: ms };
  }
  list.splice(index, 0, { startMs: ms, endMs, text: "" });
  return { cues: list, index };
}

export function cueError(cues: CaptionCue[], duration: number) {
  for (const [i, c] of cues.entries()) {
    if (!Number.isSafeInteger(c.startMs) || !Number.isSafeInteger(c.endMs) ||
      c.startMs < (cues[i - 1]?.endMs ?? 0) || c.startMs >= c.endMs || c.endMs > duration)
      return (i + 1) + "번째 자막의 시작과 끝을 구간 안에서 겹치지 않게 맞춰 주세요.";
    if (!c.text.trim()) return (i + 1) + "번째 자막의 문구를 입력해 주세요.";
  }
  return "";
}

export type CaptionDraft = { sceneId: string; revision: number; cues: CaptionCue[]; index: number };

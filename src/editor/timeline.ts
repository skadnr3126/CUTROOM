import { sceneClips, type Scene, type VisualPlacement } from "./project";

export type TimelinePiece = Pick<Scene, "id" | "revision" | "durationMs" | "audio" | "caption"> & {
  visual: VisualPlacement | null;
};

export function timelineSections(scenes: Scene[]) {
  let cursor = 0;
  return scenes.map((scene, index) => {
    const start = cursor;
    const clips = sceneClips(scene);
    const pieces: {scene: TimelinePiece; index: number; start: number; end: number; durationMs: number; sectionStart: number; owner: Scene}[] = [];
    function append(id: string, visual: VisualPlacement | null, durationMs: number) {
      const piece = {id, visual, durationMs, revision: scene.revision, audio: scene.audio, caption: scene.caption};
      pieces.push({scene: piece, index, start: cursor, end: cursor + durationMs, durationMs, sectionStart: start, owner: scene});
      cursor += durationMs;
    }
    for (const clip of clips) {
      if (clip.gapMs) append(clip.id + ":gap", null, clip.gapMs);
      append(clip.id, clip, clip.durationMs);
    }
    if (!clips.length) append(scene.id, null, scene.durationMs);
    const overlayEnd = (scene.overlays ?? []).reduce((end, layer) => Math.max(end, start + layer.startMs + layer.durationMs), start);
    if (overlayEnd > cursor) {
      const durationMs = overlayEnd - cursor;
      pieces.push({ scene: { id: scene.id + ":tail", revision: scene.revision, durationMs, audio: scene.audio,
        caption: scene.caption, visual: null }, index, start: cursor, end: overlayEnd, durationMs, sectionStart: start, owner: scene });
      cursor = overlayEnd;
    }
    return { scene, index, start, end: cursor, durationMs: cursor - start, pieces };
  });
}

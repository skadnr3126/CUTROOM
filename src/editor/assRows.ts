import type { CaptionCue, Scene } from "./project";
import { invoke } from "./projectSession";

export type AssRow = CaptionCue & { line: number };
export type AssTrack = NonNullable<Scene["captionAss"]>;

const parsedTracks = new WeakMap<AssTrack, Promise<AssRow[]>>();

export function readAssRows(track: AssTrack) {
  let rows = parsedTracks.get(track);
  if (!rows) {
    rows = invoke<AssRow[]>("ass_rows", { source: track.source }).catch(error => {
      parsedTracks.delete(track);
      throw error;
    });
    parsedTracks.set(track, rows);
  }
  return rows;
}

import type { Overlay, Scene } from "./project";

type Placement = { layer: Overlay; start: number };
type Entry = Placement & { key: string; next?: Placement };

export function overlayPlaybackRuns(sections: { scene: Scene; start: number }[]) {
  const scenes = new Map<string, Entry[]>();
  const tracks = new Map<number, Entry[]>();
  for (const { scene, start } of sections) {
    // Preserve the stored paint order, including legacy implicit tracks.
    const entries = (scene.overlays ?? []).map((layer, index) => {
      const entry: Entry = { layer, key: scene.id + ":" + layer.id, start: start + layer.startMs };
      const track = layer.track ?? index + 1;
      const items = tracks.get(track);
      if (items) items.push(entry);
      else tracks.set(track, [entry]);
      return entry;
    });
    scenes.set(scene.id, entries);
  }
  for (const entries of tracks.values()) {
    entries.sort((a, b) => a.start - b.start);
    for (let i = 1; i < entries.length; i++) {
      const previous = entries[i - 1], current = entries[i];
      if (previous.start + previous.layer.durationMs === current.start) {
        current.key = previous.key;
        previous.next = current;
      }
    }
  }
  return scenes;
}

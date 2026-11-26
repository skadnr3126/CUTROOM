import { useEffect, useState } from "react";
import { readAssRows } from "./assRows";
import { sceneCaptions } from "./captionCues";
import type { timelineSections } from "./timeline";

type Sections = ReturnType<typeof timelineSections>;

export function useCaptionStarts(sections: Sections, start: number, end: number) {
  const [index, setIndex] = useState<{ sections: Sections; start: number; end: number; times: number[] }>();
  useEffect(() => {
    let active = true;
    async function load() {
      const times = new Set<number>();
      for (const section of sections) {
        if (section.end <= start || section.start >= end) continue;
        const track = section.scene.captionAss;
        const cues = track ? await readAssRows(track).catch(() => []) : sceneCaptions(section.scene);
        if (!active) return;
        for (const cue of cues) {
          const time = section.start + cue.startMs;
          if (time >= start && time < Math.min(end, section.end)) times.add(time);
        }
      }
      setIndex({ sections, start, end, times: [...times].sort((a,b) => a-b) });
    }
    void load();
    return () => { active = false; };
  }, [sections, start, end]);
  return index?.sections === sections && index.start === start && index.end === end ? index.times : [];
}

export function adjacentCaptionStarts(times: number[], position: number) {
  let lo = 0, hi = times.length;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (times[mid] < position) lo = mid + 1;
    else hi = mid;
  }
  return { previous: times[lo - 1], next: times[times[lo] === position ? lo + 1 : lo] };
}

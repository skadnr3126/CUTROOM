import { useState } from "react";
import type { CaptionCue } from "./project";
import { TimeGrip } from "./TimeGrip";
import { sourceTime } from "./videoTrim";
import type { MenuAction } from "./ClipMenu";

type Props = {
  cues: CaptionCue[]; durationMs: number; scale: number; busy: boolean;
  precision?: number; label?: string; allowAdd?: boolean;
  remove: (cue: CaptionCue) => void; menu: (action: MenuAction) => void;
  open: (ms: number, index?: number) => void;
  seek: (ms: number) => void;
  save: (cues: CaptionCue[]) => void;
};

export function CaptionTrack({ cues, durationMs, scale, busy, open, seek, save, remove, menu, precision = 1,
  label = "자막 트랙", allowAdd = false }: Props) {
  const [drag, setDrag] = useState<CaptionCue[] | null>(null);
  return <div className="track-row captions cue-track" aria-label={label}
    onDoubleClick={e => {
      if (allowAdd && !busy && e.target === e.currentTarget) {
        const ms = Math.round((e.clientX - e.currentTarget.getBoundingClientRect().left) / scale);
        if (ms < durationMs) open(ms);
      }
    }}>
    {(drag ?? cues).map((display, index) => {
      const cue = cues[index];
      const previous = cues[index - 1];
      const following = cues[index + 1];
      const limit = Math.floor(durationMs / precision) * precision;
      function adjust(part: "move" | "start" | "end", delta: number, commit: boolean) {
        // Extending across a shared edge borrows time from the adjoining cue.
        // Imported ASS layers can already overlap; retain that freedom for those layers.
        const left = part === "start" && previous?.endMs === cue.startMs ? previous.startMs + precision :
          previous && previous.endMs <= cue.startMs ? previous.endMs : 0;
        const right = part === "end" && following?.startMs === cue.endMs ? following.endMs - precision :
          following && following.startMs >= cue.endMs ? following.startMs : limit;
        const min = part === "end" ? cue.startMs + precision - cue.endMs : left - cue.startMs;
        const max = part === "start" ? cue.endMs - precision - cue.startMs : right - cue.endMs;
        const shift = Math.min(max, Math.max(min, Math.round(delta / precision) * precision));
        const changed = { ...cue, startMs: cue.startMs + (part === "end" ? 0 : shift), endMs: cue.endMs + (part === "start" ? 0 : shift) };
        const next = cues.slice();
        next[index] = changed;
        if (part === "start" && shift < 0 && previous?.endMs === cue.startMs)
          next[index-1] = { ...previous, endMs: changed.startMs };
        if (part === "end" && shift > 0 && following?.startMs === cue.endMs)
          next[index+1] = { ...following, startMs: changed.endMs };
        if (commit) { setDrag(null); if (shift) save(next); }
        else setDrag(next);
        seek(part === "end" ? changed.endMs - 1 : changed.startMs);
      }
      return <div className="cue-block" key={index} style={{ left: display.startMs * scale, width: (display.endMs - display.startMs) * scale }}
        onDoubleClick={e => { e.stopPropagation(); if (!busy) open(cue.startMs, index); }}
        onContextMenu={e => {
          e.preventDefault(); e.stopPropagation();
          if (busy) return;
          seek(cue.startMs);
          menu({ x: e.clientX, y: e.clientY, items: [{ label: "자막 삭제", run: () => remove(cue) }] });
        }}
        aria-keyshortcuts="F2 Delete" title="양끝 드래그로 길이 조절 · F2로 편집 · Delete로 삭제"
        onKeyDown={e => { if ((e.key === "Enter" || e.key === "F2") && !busy && !e.repeat) {
          e.preventDefault(); e.stopPropagation(); open(cue.startMs, index);
        } }}>
        <TimeGrip className="clip filled cue-body" label={cue.text + " · " + sourceTime(cue.startMs) + "–" + sourceTime(cue.endMs) + " · F2로 편집"}
          scale={scale} disabled={busy} onSelect={() => seek(cue.startMs)} onCancel={() => setDrag(null)}
          onDelete={() => remove(cue)} onDelta={(delta, commit) => adjust("move",delta,commit)}>{display.text}</TimeGrip>
        {(["start","end"] as const).map(edge => <TimeGrip key={edge} className={"cue-edge " + edge} scale={scale} disabled={busy}
          label={(index + 1) + "번째 자막 " + (edge === "start" ? "시작" : "끝") + " 조절"}
          onSelect={() => seek(edge === "start" ? cue.startMs : cue.endMs - 1)}
          onDelete={() => remove(cue)} onDelta={(delta, commit) => adjust(edge,delta,commit)} onCancel={() => setDrag(null)} />)}
      </div>;
    })}
  </div>;
}

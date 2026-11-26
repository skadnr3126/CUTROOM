import { useMemo } from "react";
import type { CaptionCue } from "./project";
import { CaptionTrack } from "./CaptionTrack";
import type { AssRow } from "./assRows";
import type { MenuAction } from "./ClipMenu";

type Group = CaptionCue & { rows: AssRow[] };

function groupRows(rows: AssRow[]): Group[] {
  const groups = new Map<string, Group>();
  for (const row of rows) {
    const key = row.startMs + ":" + row.endMs;
    const group = groups.get(key);
    if (group) {
      group.rows.push(row);
      group.text += "\n" + row.text;
    } else {
      groups.set(key, { ...row, rows: [row] });
    }
  }
  return [...groups.values()].sort((a, b) => a.startMs - b.startMs || a.endMs - b.endMs);
}

export function AssCaptionTrack({ rows, durationMs, scale, busy, seek, edit, save, remove, menu }: {
  rows: AssRow[]; durationMs: number; scale: number; busy: boolean;
  seek: (ms: number) => void; edit: (ms: number, line: number) => void;
  save: (rows: AssRow[]) => void;
  remove: (cue: CaptionCue) => void; menu: (action: MenuAction) => void;
}) {
  const groups = useMemo(() => groupRows(rows), [rows]);
  return <CaptionTrack cues={groups} durationMs={durationMs} scale={scale} busy={busy} seek={seek} menu={menu}
    remove={cue=>{
      const group=groups.find(g=>g.startMs===cue.startMs && g.endMs===cue.endMs);
      if(group && group.rows.length>1) edit(cue.startMs,group.rows[0].line);
      else remove(cue);
    }}
    precision={10} label="ASS 자막 트랙" open={(ms, i) => { if (i !== undefined) edit(ms, groups[i].rows[0].line); }}
    save={next => {
      const edits = next.flatMap(({ startMs, endMs }, i) => {
        const group = groups[i];
        if (startMs === group.startMs && endMs === group.endMs) return [];
        return group.rows.map(row => ({ ...row, startMs, endMs }));
      });
      if (edits.length) save(edits);
    }} />;
}

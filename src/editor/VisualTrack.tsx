import { useState, type DragEvent } from "react";
import { Filmstrip } from "./Filmstrip";
import { TimeGrip } from "./TimeGrip";
import { sourceDragType, sourceUrl, type Asset } from "./project";
import { videoRange } from "./videoTrim";
import { endOf, type TrackClip } from "./visualTracks";

const clipDrag = "application/x-cutroom-track-clip";

type Props = {
  track: number; fresh: boolean; clips: TrackClip[]; selected: string | null; sceneId: string;
  scale: number; viewport: { left: number; right: number }; assets: Map<string, Asset>; mediaDir: string;
  busy: boolean; measured: Record<string, number>;
  select: (clip: TrackClip, at: number) => void; remove: (clip: TrackClip) => void;
  save: (clip: TrackClip, x: number, y: number) => void;
  resize: (clip: TrackClip, edge: "start" | "end", delta: number, commit: boolean) => void;
  cancel: () => void;
  drop: (track: number, at: number, item: Asset | TrackClip, target?: string, after?: boolean) => void;
};

export function VisualTrack({ track, fresh, clips, selected, sceneId, scale, viewport, assets, mediaDir, busy, measured, select, remove, save, resize, cancel, drop }: Props) {
  const [hover, setHover] = useState(false);
  function receive(e: DragEvent<HTMLDivElement>) {
    setHover(false);
    if (busy) return;
    const asset = assets.get(e.dataTransfer.getData(sourceDragType));
    const [origin, id, grip] = e.dataTransfer.getData(clipDrag).split("|");
    const moving = origin === sceneId ? clips.find(c => c.id === id) : undefined;
    if ((!asset && !moving) || asset?.kind === "audio") return;
    e.preventDefault(); e.stopPropagation();
    const at = (e.clientX - e.currentTarget.getBoundingClientRect().left) / scale;
    const target = clips.find(c => c.track === track && at >= c.startMs && at < endOf(c));
    drop(track, Math.max(0, at - (moving ? Number(grip) || 0 : 0)), (asset ?? moving)!, target?.id,
      !!target && at >= target.startMs + target.durationMs / 2);
  }
  return <div className={"track-row visual" + (hover ? " drop-target" : "")} data-track={track} onDrop={receive}
    onDragLeave={e => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setHover(false); }}
    onDragOver={e => {
      if (!busy && [sourceDragType, clipDrag].some(t => e.dataTransfer.types.includes(t))) {
        e.preventDefault(); e.dataTransfer.dropEffect = e.dataTransfer.types.includes(clipDrag) ? "move" : "copy"; setHover(true);
      }
    }}>
    {fresh && <span className="new-track-hint">클립을 놓아 트랙 추가</span>}
    {clips.filter(c => c.track === track).map(c => {
      const asset = assets.get(c.assetId);
      if (!asset) return null;
      const video = asset.kind === "video";
      const range = videoRange(c, c.durationMs, asset.durationMs ?? measured[asset.id]);
      const left = (c.startMs + (video ? range.offsetMs : 0)) * scale;
      const width = (video ? range.outMs - range.inMs : c.durationMs) * scale;
      return <div key={c.id} className="track-slot visual-slot" style={{ left, width }} data-clip={c.id}
        onContextMenu={e => { e.preventDefault(); if (!busy) { select(c, c.startMs); save(c, e.clientX, e.clientY); } }}>
        <button className={"clip filled" + (selected === c.id ? " picked active" : "")} aria-label={asset.name} title={asset.name}
          aria-pressed={selected === c.id} aria-keyshortcuts="Delete" draggable={!busy}
          onDragStart={e => { e.dataTransfer.setData(clipDrag, [sceneId, c.id, (e.clientX - e.currentTarget.getBoundingClientRect().left) / scale].join("|")); e.dataTransfer.effectAllowed = "move"; }}
          onClick={e => select(c, c.startMs + (video ? range.offsetMs : 0) + (e.detail ? Math.min(c.durationMs - 1, Math.max(0, Math.round((e.clientX - e.currentTarget.getBoundingClientRect().left) / scale))) : 0))}
          onKeyDown={e => { if (e.key === "Delete" && !busy && !e.repeat) { e.preventDefault(); remove(c); } }}>
          {video ? <Filmstrip assetId={asset.id} inMs={range.inMs} outMs={range.outMs} pixelsPerMs={scale}
            visibleFrom={viewport.left - left} visibleTo={viewport.right - left} /> :
            <img className="still-clip" src={sourceUrl(mediaDir, asset)} alt="" draggable={false} />}
          <span className="filmstrip-name">{asset.name}</span>
        </button>
        {(["start", "end"] as const).map(edge => <TimeGrip key={edge} className={"trim-handle " + edge}
          style={{ left: edge === "start" ? 0 : "100%" }} label={asset.name + " " + (edge === "start" ? "시작점" : "끝점")}
          homeDelta={video && edge === "start" ? -range.inMs : undefined}
          endDelta={video && edge === "end" ? (asset.durationMs ?? measured[asset.id] ?? range.outMs) - range.outMs : undefined}
          scale={scale} disabled={busy} onSelect={() => select(c, edge === "start" ? c.startMs : endOf(c) - 1)}
          onCancel={cancel} onDelete={() => remove(c)} onDelta={(delta, commit) => resize(c, edge, delta, commit)} />)}
      </div>;
    })}
  </div>;
}

import { Filmstrip } from "./Filmstrip";
import { sceneLabel, sourceUrl, type Asset } from "./project";
import type { timelineSections } from "./timeline";
import { videoRange } from "./videoTrim";

type Props = {
  side: "앞" | "뒤";
  section: ReturnType<typeof timelineSections>[number];
  start: number;
  end: number;
  time: number;
  scale: number;
  assets: Map<string, Asset>;
  mediaDir: string;
  viewport: { left: number; right: number };
  seek: (time: number) => void;
};

export function NeighborPreviewTrack({ side, section, start, end, time, scale, assets, mediaDir, viewport, seek }: Props) {
  const length = end - start;
  if (length <= 0) return null;
  const name = sceneLabel(section.scene, section.index);
  const label = side + " 구간 · " + name;
  return <div className="neighbor-track" style={{ width: length * scale }} title={label}>
    <strong>{side} 구간</strong>
    <span className="neighbor-name">{name}</span>
    <div className="neighbor-frames" aria-hidden="true">
      {section.pieces.map(piece => {
        const clip = piece.scene.visual;
        const asset = clip && assets.get(clip.assetId);
        if (!clip || !asset || piece.start >= end || piece.end <= start) return null;
        const range = asset.kind === "video" ? videoRange(clip, piece.durationMs, asset.durationMs)
          : { inMs: 0, outMs: piece.durationMs, offsetMs: 0 };
        const origin = piece.start + range.offsetMs;
        const from = Math.max(start, origin);
        const to = Math.min(end, origin + range.outMs - range.inMs);
        if (to <= from) return null;
        const left = (from - start) * scale;
        return <span key={piece.scene.id} className="neighbor-piece" style={{ left, width: (to - from) * scale }}>
          {asset.kind === "video" ? <Filmstrip assetId={asset.id} inMs={range.inMs + from - origin}
            outMs={range.inMs + to - origin} pixelsPerMs={scale}
            visibleFrom={viewport.left - left} visibleTo={viewport.right - left} /> :
            <img className="still-clip" src={sourceUrl(mediaDir, asset)} alt="" />}
        </span>;
      })}
    </div>
    <input type="range" className="neighbor-seek" aria-label={label + " 미리보기 위치"} min={0} max={length - 1}
      step={1} value={Math.max(0, Math.min(length - 1, Math.round(time - start)))}
      onPointerDown={e => {
        if (e.button !== 0) return;
        const bounds = e.currentTarget.getBoundingClientRect();
        seek(start + Math.min(length - 1, Math.max(0, Math.round((e.clientX - bounds.left) / scale))));
      }}
      onChange={e => seek(start + e.target.valueAsNumber)} />
  </div>;
}

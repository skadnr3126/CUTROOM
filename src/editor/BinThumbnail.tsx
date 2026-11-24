import { memo, useEffect, useRef, useState } from "react";
import { Thumbnail } from "./Filmstrip";
import { sourceUrl, timelineTime, videoDuration, type Asset } from "./project";

export type BinMode = "grid" | "list";

export const BinThumbnail = memo(function BinThumbnail({ asset, mediaDir, mode }: {
  asset: Asset; mediaDir: string; mode: BinMode;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const [visible, setVisible] = useState(false);
  const [width, setWidth] = useState(0);
  const [measured, setMeasured] = useState<number | null>(null);
  useEffect(() => {
    const el = ref.current!;
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting),
      { root: el.closest(".source-list"), rootMargin: "80px 0px" });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!visible || mode !== "list") return;
    const observer = new ResizeObserver(([entry]) => setWidth(Math.round(entry.contentRect.width)));
    observer.observe(ref.current!);
    return () => observer.disconnect();
  }, [mode, visible]);
  useEffect(() => {
    if (!visible || asset.kind !== "video" || asset.durationMs != null || measured != null) return;
    let active = true;
    void videoDuration(mediaDir, asset).then(ms => { if (active) setMeasured(ms); });
    return () => { active = false; };
  }, [asset, mediaDir, visible, measured]);
  const duration = asset.durationMs ?? measured;
  const count = mode === "grid" || (asset.kind === "video" && !duration) ? 1 : Math.max(2, Math.ceil(width / 85));
  const url = sourceUrl(mediaDir, asset);
  return <span ref={ref} className={`bin-thumb ${asset.kind}`} aria-hidden="true">
    <span className="bin-placeholder">{asset.kind === "audio" ? "♪" : asset.kind === "video" ? "▷" : "▧"}</span>
    {visible && asset.kind !== "audio" && <span className="bin-frames">
      {Array.from({ length: count }, (_, i) => {
        const at = duration ? Math.floor(duration * (i + .5) / count) : 0;
        return asset.kind === "video"
          ? <Thumbnail key={at + "-" + i} assetId={asset.id} atMs={at} fallbackUrl={url} />
          : <span key={i}><img src={url} alt="" draggable={false} /></span>;
      })}
    </span>}
    {duration != null && duration > 0 && <small className="bin-duration">{timelineTime(duration)}</small>}
  </span>;
});

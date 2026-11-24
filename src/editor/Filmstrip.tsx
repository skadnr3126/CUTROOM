import { invoke } from "./projectSession";
import { memo, useEffect, useState } from "react";

const frames = new Map<string, string>();
type Job = { assetId: string; atMs: number; listeners: Set<(url: string) => void>; started: boolean };
const jobs = new Map<string, Job>();
let active = 0;

function pump() {
  for (const [key, job] of jobs) {
    if (active >= 2) break;
    if (job.started) continue;
    job.started = true;
    active++;
    void invoke<string>("project_clip_thumbnail", { assetId: job.assetId, atMs: job.atMs })
      .then((url) => {
        frames.set(key, url);
        if (frames.size > 384) frames.delete(frames.keys().next().value!);
        job.listeners.forEach((show) => show(url));
      })
      .catch(() => job.listeners.forEach(show => show("")))
      .finally(() => { jobs.delete(key); active--; pump(); });
  }
}

export function Thumbnail({ assetId, atMs, width, fallbackUrl }: { assetId: string; atMs: number; width?: number; fallbackUrl?: string }) {
  const key = assetId + ":" + atMs;
  const [url, setUrl] = useState(() => frames.get(key));
  useEffect(() => {
    const cached = frames.get(key);
    if (cached) { setUrl(cached); return; }
    let job: Job | undefined;
    const timer = window.setTimeout(() => {
      const cached = frames.get(key);
      if (cached) { setUrl(cached); return; }
      job = jobs.get(key) ?? { assetId, atMs, started: false, listeners: new Set() };
      job.listeners.add(setUrl);
      jobs.set(key, job);
      pump();
    }, 55);
    return () => {
      clearTimeout(timer);
      job?.listeners.delete(setUrl);
      if (job && !job.started && !job.listeners.size) jobs.delete(key);
    };
  }, [assetId, atMs, key]);
  return <span style={{ width }}>{url ? <img src={url} alt="" draggable={false} /> : url === "" && fallbackUrl ?
    <video src={fallbackUrl + "#t=" + atMs / 1000} preload="metadata" muted playsInline /> : null}</span>;
}

type Props = {
  assetId: string; inMs: number; outMs: number; pixelsPerMs: number;
  visibleFrom: number; visibleTo: number;
};

export const Filmstrip = memo(function Filmstrip({ assetId, inMs, outMs, pixelsPerMs, visibleFrom, visibleTo }: Props) {
  // Keep the tile grid anchored to source time while the edit boundaries move.
  const step = Math.max(1, Math.round(72 / pixelsPerMs));
  const from = Math.max(inMs, inMs + visibleFrom / pixelsPerMs - step);
  const to = Math.min(outMs, inMs + visibleTo / pixelsPerMs + step);
  if (from >= to) return null;
  const first = Math.floor(from / step) * step;
  const tiles = [];
  for (let time = first; time < to; time += step) {
    const start = Math.max(inMs, time);
    const end = Math.min(outMs, time + step);
    const atMs = Math.floor((start + end) / 2);
    tiles.push(<Thumbnail key={assetId + ":" + atMs} assetId={assetId} atMs={atMs} width={(end - start) * pixelsPerMs} />);
  }
  return <span className="filmstrip" aria-hidden="true" style={{ left: Math.max(0, first - inMs) * pixelsPerMs }}>{tiles}</span>;
});

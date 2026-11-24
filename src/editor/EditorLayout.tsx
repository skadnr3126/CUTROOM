import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { PanelDivider } from "./PanelDivider";
import { VerticalSplit } from "./VerticalSplit";

const gutter = 10;
function useContentSize() {
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setSize(old => width === old.width && height === old.height ? old : { width, height });
    });
    observer.observe(ref.current!);
    return () => observer.disconnect();
  }, []);
  return { ref, ...size };
}

type Track = { weight: number; min: number; label: string; id: string };
const columns: Track[] = [
  { weight: 73, min: 226, label: "작업 영역과 대본 사이 너비", id: "stage-top" },
  { weight: 27, min: 170, label: "", id: "script-pane" },
];
const stageColumns: Track[] = [
  { weight: 32, min: 148, label: "소스 보관함과 미리보기 사이 너비", id: "source-pane" },
  { weight: 68, min: 180, label: "", id: "preview-pane" },
];
const stageRows: Track[] = [
  { ...stageColumns[0], weight: 47, min: 110, label: "소스 보관함과 미리보기 사이 높이" },
  { ...stageColumns[1], weight: 53, min: 110 },
];

function useTracks(tracks: Track[], preference: string, extent: number, axis: "x" | "y") {
  const [weights, setWeights] = useState<number[]>(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(preference) ?? "null");
      if (Array.isArray(saved) && saved.length === tracks.length && saved.every(x => Number.isFinite(x) && x > 0 && x <= 100))
        return saved;
      if (saved === null && tracks === columns) {
        const previous = JSON.parse(localStorage.getItem("cutroom.panel-columns.v1") ?? "null");
        if (Array.isArray(previous) && previous.length === 3 && previous.every(x => Number.isFinite(x) && x > 0 && x <= 100))
          return previous.slice(0, 2);
      }
    } catch { /* Use the default dock proportions. */ }
    return tracks.map(t => t.weight);
  });
  const available = Math.max(1, extent - (tracks.length - 1) * gutter);
  const minTotal = tracks.reduce((sum, t) => sum + t.min, 0);
  const floors = tracks.map(t => t.min * Math.min(1, available * .95 / minTotal));
  // Respect each pane's minimum and distribute the rest by saved proportions.
  const sizes = weights.map(() => 0);
  let rest = available, weight = weights.reduce((a, b) => a + b, 0);
  const pending = new Set(weights.keys());
  for (const i of [...pending].sort((a, b) => floors[b] / weights[b] - floors[a] / weights[a])) {
    if (weights[i] * rest / weight >= floors[i]) break;
    sizes[i] = floors[i];
    rest -= floors[i];
    weight -= weights[i];
    pending.delete(i);
  }
  for (const i of pending) sizes[i] = rest * weights[i] / weight;
  const template = sizes.map(n => n + "px").join(" " + gutter + "px ");
  const dividers = tracks.slice(0, -1).map((track, i) => {
    const pair = sizes[i] + sizes[i + 1];
    return <PanelDivider key={preference + i} className={`dock-divider dock-divider-${i} axis-${axis}`}
      controls={track.id + " " + tracks[i + 1].id} label={track.label} axis={axis}
      value={100 * sizes[i] / pair} minimum={100 * floors[i] / pair} maximum={100 - 100 * floors[i + 1] / pair}
      perPixel={() => 100 / pair} onResize={(value, commit) => {
        const next = [...sizes];
        next[i] = pair * value / 100;
        next[i + 1] = pair - next[i];
        const ratios = next.map(n => 100 * n / available);
        setWeights(ratios);
        if (commit) try { localStorage.setItem(preference, JSON.stringify(ratios)); } catch { /* Keep the live layout. */ }
      }} />;
  });
  return { template, dividers };
}

export function EditorLayout({ children }: { children: ReactNode }) {
  const { ref, width } = useContentSize();
  const tracks = useTracks(columns, "cutroom.panel-columns.v2", width, "x");
  return <VerticalSplit className="editor-layout" label="타임라인 높이" controls="studio-timeline"
    layoutRef={ref} style={{ gridTemplateColumns: width ? tracks.template : undefined }}
    preference="cutroom.timeline-height.v1" initial={31} minimum={20} maximum={65}>
    {children}
    {tracks.dividers}
  </VerticalSplit>;
}

export function StageLayout({ children }: { children: ReactNode }) {
  const { ref, width, height } = useContentSize();
  const wide = useTracks(stageColumns, "cutroom.source-preview-width.v1", width, "x");
  const narrow = useTracks(stageRows, "cutroom.source-preview-height.v1", height, "y");
  const stacked = width > 0 && width < 425;
  const tracks = stacked ? narrow : wide;
  const style: CSSProperties = stacked ? { gridTemplateRows: tracks.template } :
    { gridTemplateColumns: width ? tracks.template : undefined };
  return <div ref={ref} className={`stage-top${stacked ? " stacked" : ""}`} id="stage-top" style={style}>
    {children}
    {tracks.dividers}
  </div>;
}

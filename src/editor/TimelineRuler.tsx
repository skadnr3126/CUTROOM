import { timelineTime } from "./project";
import { sourceTime } from "./videoTrim";

type Props = {
  pixelsPerMs: number;
  viewport: { left: number; right: number };
  duration: number;
  time: number;
  onSeek: (ms: number) => void;
  label: string;
};

export function TimelineRuler({ pixelsPerMs, viewport, duration, time, onSeek, label }: Props) {
  // Keep labels legible at every zoom level and only render the visible ticks.
  const minimumStep = Math.max(1, 72 / pixelsPerMs);
  const magnitude = 10 ** Math.floor(Math.log10(minimumStep));
  const step = [1, 2, 5, 10].map(n => n * magnitude).find(ms => ms >= minimumStep)!;
  const first = Math.max(0, Math.floor(viewport.left / (step * pixelsPerMs)));
  const last = Math.floor(Math.min(duration, viewport.right / pixelsPerMs) / step);
  const ticks = [];
  for (let i = first; i <= last; i++) {
    const ms = i * step;
    const fraction = String(ms % 1000).padStart(3, "0").slice(0, Math.max(0, 3 - Math.floor(Math.log10(step))));
    ticks.push(<span key={ms} style={{ left: ms * pixelsPerMs }}>
      {timelineTime(ms)}{fraction && "." + fraction}
    </span>);
  }
  return <div className="timeline-scale">
    {ticks}
    {duration > 0 && <input className="ruler-seek" type="range" min={0} max={duration} step={1}
      style={{ width: duration * pixelsPerMs }} value={Math.round(time)}
      aria-label={label} aria-valuetext={sourceTime(time)}
      onChange={event => onSeek(event.target.valueAsNumber)} />}
  </div>;
}

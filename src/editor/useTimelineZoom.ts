import { useLayoutEffect, useRef, useState } from "react";

type Scope = "all" | "detail";
type ZoomSetting = number | "fit" | null;

const defaultScale = 0.08;
// Chromium limits the layout width of a single element to about 33.5 million pixels.
const maxTimelineWidth = 30_000_000;

export function useTimelineZoom(scope: Scope, durationMs: number, playheadMs: number, leadingPx: number) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState({ left: 0, right: 0 });
  const [settings, setSettings] = useState<Record<Scope, ZoomSetting>>({ all: null, detail: null });
  const anchor = useRef<{ scope: Scope; timeMs: number; offsetPx: number } | null>(null);

  useLayoutEffect(() => {
    const node = scrollRef.current;
    if (!node) return;
    const measure = () => {
      const left = node.scrollLeft;
      const right = left + Math.max(0, node.clientWidth - leadingPx);
      setViewport(old => old.left === left && old.right === right ? old : { left, right });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    node.addEventListener("scroll", measure, { passive: true });
    return () => {
      observer.disconnect();
      node.removeEventListener("scroll", measure);
    };
  }, [scope, leadingPx]);

  const width = viewport.right - viewport.left;
  const fitScale = durationMs > 0 ? Math.max(1, width) / durationMs : defaultScale;
  const minScale = Math.min(defaultScale / 8, fitScale / 2);
  const maxScale = Math.min(Math.max(1.6, fitScale * 2), maxTimelineWidth / Math.max(durationMs, 1));
  const setting = settings[scope];
  const isFit = setting === "fit";
  const initialScale = scope === "all" ? Math.min(defaultScale, fitScale) : defaultScale;
  const pixelsPerMs = Math.max(minScale, Math.min(maxScale, isFit ? fitScale : setting ?? initialScale));
  const logRange = Math.log(maxScale / minScale);
  const sliderValue = 100 * Math.log(pixelsPerMs / minScale) / logRange;

  useLayoutEffect(() => {
    const target = anchor.current;
    if (target && target.scope === scope && scrollRef.current) {
      scrollRef.current.scrollLeft = target.timeMs * pixelsPerMs - target.offsetPx;
    }
    anchor.current = null;
  }, [pixelsPerMs, scope]);

  function setSliderValue(value: number) {
    const scale = minScale * Math.exp(logRange * Math.max(0, Math.min(100, value)) / 100);
    const node = scrollRef.current;
    if (node && scale !== pixelsPerMs) {
      const visibleWidth = Math.max(0, node.clientWidth - leadingPx);
      const headOffset = playheadMs * pixelsPerMs - node.scrollLeft;
      const offsetPx = headOffset >= 0 && headOffset <= visibleWidth ? headOffset : visibleWidth / 2;
      anchor.current = { scope, timeMs: (node.scrollLeft + offsetPx) / pixelsPerMs, offsetPx };
    }
    setSettings(old => ({ ...old, [scope]: scale }));
  }

  function fit() {
    anchor.current = null;
    if (scrollRef.current) scrollRef.current.scrollLeft = 0;
    setSettings(old => ({ ...old, [scope]: "fit" }));
  }

  return { scrollRef, viewport, pixelsPerMs, sliderValue, setSliderValue, isFit, fit };
}

import { useRef, type PointerEvent } from "react";

type Props = {
  className: string;
  label: string;
  controls: string;
  axis: "x" | "y";
  value: number;
  minimum: number;
  maximum: number;
  // Percentage change per CSS pixel, negative when resizing the trailing pane.
  perPixel: () => number;
  onResize: (value: number, commit: boolean) => void;
};

export function PanelDivider({ className, label, controls, axis, value, minimum, maximum, perPixel, onResize }: Props) {
  const drag = useRef<{ id: number; apply: (at: number, commit: boolean) => void; cancel: () => void } | null>(null);
  const coordinate = (e: PointerEvent) => axis === "x" ? e.clientX : e.clientY;
  const clamp = (v: number) => Math.min(maximum, Math.max(minimum, v));
  function cancel(e: PointerEvent) {
    if (drag.current?.id !== e.pointerId) return;
    drag.current.cancel();
    drag.current = null;
  }
  return <div className={className} role="separator" aria-label={label} aria-controls={controls}
    aria-orientation={axis === "x" ? "vertical" : "horizontal"} aria-valuemin={Math.round(minimum)}
    aria-valuemax={Math.round(maximum)} aria-valuenow={Math.round(value)} aria-valuetext={Math.round(value) + "%"}
    tabIndex={0} title={label + " 조절 (드래그 또는 방향키)"}
    onPointerDown={e => {
      if (e.button !== 0 || drag.current) return;
      const rate = perPixel();
      if (!Number.isFinite(rate) || !rate) return;
      e.preventDefault();
      e.currentTarget.focus({ preventScroll: true });
      e.currentTarget.setPointerCapture(e.pointerId);
      const start = coordinate(e);
      drag.current = { id: e.pointerId,
        apply: (at, commit) => onResize(clamp(value + (at - start) * rate), commit),
        cancel: () => onResize(value, false) };
    }}
    onPointerMove={e => { if (drag.current?.id === e.pointerId) drag.current.apply(coordinate(e), false); }}
    onPointerUp={e => {
      if (drag.current?.id !== e.pointerId) return;
      drag.current.apply(coordinate(e), true);
      drag.current = null;
    }}
    onPointerCancel={cancel} onLostPointerCapture={cancel}
    onKeyDown={e => {
      const step = (e.shiftKey ? 10 : 2) * Math.sign(perPixel());
      const increase = axis === "x" ? "ArrowRight" : "ArrowDown";
      const decrease = axis === "x" ? "ArrowLeft" : "ArrowUp";
      if (![increase, decrease, "Home", "End"].includes(e.key)) return;
      e.preventDefault();
      onResize(clamp(e.key === "Home" ? minimum : e.key === "End" ? maximum :
        value + (e.key === increase ? step : -step)), true);
    }} />;
}

import { useRef, type CSSProperties, type PointerEvent, type ReactNode } from "react";

type Props = {
  label: string; scale: number; className: string; style?: CSSProperties; disabled: boolean; children?: ReactNode;
  homeDelta?: number; endDelta?: number;
  onSelect: () => void; onDelete?: () => void;
  onDelta: (ms: number, commit: boolean) => void; onCancel: () => void;
};

// Keep the original timing and scale throughout a drag, even as the row reflows.
export function TimeGrip({ label, scale, className, style, disabled, children, homeDelta, endDelta, onSelect, onDelta, onCancel, onDelete }: Props) {
  const gesture = useRef<{ id: number; x: number; scale: number; apply: Props["onDelta"] } | null>(null);
  function cancel(e: PointerEvent) {
    if (gesture.current?.id !== e.pointerId) return;
    gesture.current = null;
    onCancel();
  }
  return <button type="button" aria-label={label} title={label} className={className} style={style} disabled={disabled}
    onPointerDown={e => {
      if (e.button !== 0 || gesture.current) return;
      e.preventDefault(); e.currentTarget.focus({ preventScroll: true });
      e.currentTarget.setPointerCapture(e.pointerId);
      gesture.current = { id: e.pointerId, x: e.clientX, scale, apply: onDelta };
      onSelect();
    }}
    onPointerMove={e => {
      const g = gesture.current;
      if (g?.id === e.pointerId) g.apply(Math.round((e.clientX - g.x) / g.scale), false);
    }}
    onPointerUp={e => {
      const g = gesture.current;
      if (g?.id !== e.pointerId) return;
      gesture.current = null;
      const delta = Math.round((e.clientX - g.x) / g.scale);
      if (delta) g.apply(delta, true); else onCancel();
    }}
    onPointerCancel={cancel} onLostPointerCapture={cancel}
    onClick={e => { if (!e.detail) onSelect(); }}
    onKeyDown={e => {
      if (e.key === "Delete" && onDelete) { e.preventDefault(); e.stopPropagation(); if (!e.repeat) onDelete(); return; }
      const step = e.shiftKey ? 500 : 100;
      const delta = {ArrowLeft:-step, ArrowRight:step, Home:homeDelta, End:endDelta}[e.key];
      if (delta === undefined) return;
      e.preventDefault(); onSelect(); onDelta(delta,true);
    }}>{children}</button>;
}

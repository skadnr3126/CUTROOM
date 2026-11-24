import { useEffect, useRef, useState, type PointerEvent } from "react";

type Rect = { left: number; top: number; width: number; height: number };
type Gesture = {
  node: HTMLDivElement; pointer: number; startX: number; startY: number;
  clientX: number; clientY: number; additive: boolean; before: string[];
  result: string[]; started: boolean; frame: number;
};

export function useFileMarquee(selected: string[], change: (ids: string[]) => void, finish: (ids: string[]) => void, busy: boolean) {
  const gesture = useRef<Gesture | null>(null);
  const [rectangle, setRectangle] = useState<Rect | null>(null);
  useEffect(() => () => {
    if (gesture.current) cancelAnimationFrame(gesture.current.frame);
    gesture.current = null;
  }, []);

  function update(g: Gesture) {
    const box = g.node.getBoundingClientRect();
    const x = Math.max(0, Math.min(g.node.clientWidth, g.clientX - box.left)) + g.node.scrollLeft;
    const y = Math.max(0, Math.min(g.node.clientHeight, g.clientY - box.top)) + g.node.scrollTop;
    if (!g.started && Math.hypot(x - g.startX, y - g.startY) < 3) return;
    const first = !g.started;
    g.started = true;
    const left = Math.min(g.startX, x), top = Math.min(g.startY, y);
    const width = Math.abs(x - g.startX), height = Math.abs(y - g.startY);
    setRectangle({ left, top, width, height });
    const screenX = box.left + left - g.node.scrollLeft;
    const screenY = box.top + top - g.node.scrollTop;
    const ids = new Set(g.additive ? g.before : []);
    for (const card of Array.from(g.node.querySelectorAll<HTMLElement>("[data-source-id]"))) {
      const r = card.getBoundingClientRect();
      if (r.right > screenX && r.left < screenX + width && r.bottom > screenY && r.top < screenY + height)
        ids.add(card.dataset.sourceId!);
    }
    const result = [...ids];
    if (first || result.length !== g.result.length || result.some((id,i) => id !== g.result[i])) {
      g.result = result;
      change(result);
    }
  }

  function scrollAtEdge(g: Gesture) {
    if (g.frame || !g.started) return;
    const box = g.node.getBoundingClientRect();
    const fromTop = g.clientY - box.top, fromBottom = box.top + g.node.clientHeight - g.clientY;
    const step = fromTop < 20 ? -Math.min(16, 20 - fromTop) : fromBottom < 20 ? Math.min(16, 20 - fromBottom) : 0;
    if (!step) return;
    g.frame = requestAnimationFrame(() => {
      g.frame = 0;
      if (gesture.current !== g) return;
      const before = g.node.scrollTop;
      g.node.scrollTop += step;
      if (g.node.scrollTop !== before) { update(g); scrollAtEdge(g); }
    });
  }

  function end(cancel: boolean) {
    const g = gesture.current;
    if (!g) return;
    gesture.current = null;
    cancelAnimationFrame(g.frame);
    if (g.node.hasPointerCapture(g.pointer)) g.node.releasePointerCapture(g.pointer);
    setRectangle(null);
    if (cancel && g.started) change(g.before);
    if (!cancel) finish(g.started ? g.result : g.additive ? g.before : []);
  }

  return {
    rectangle,
    events: {
      onPointerDown(e: PointerEvent<HTMLDivElement>) {
        const node = e.currentTarget;
        if (busy || gesture.current || e.button !== 0 || e.pointerType === "touch" || (e.target as Element).closest("button")) return;
        const box = node.getBoundingClientRect();
        const x = e.clientX - box.left, y = e.clientY - box.top;
        if (x >= node.clientWidth || y >= node.clientHeight) return;
        e.preventDefault();
        node.focus({preventScroll:true});
        node.setPointerCapture(e.pointerId);
        gesture.current = {node, pointer:e.pointerId, startX:x+node.scrollLeft, startY:y+node.scrollTop,
          clientX:e.clientX, clientY:e.clientY, additive:e.ctrlKey || e.metaKey || e.shiftKey, before:selected,
          result:[], started:false, frame:0};
      },
      onPointerMove(e: PointerEvent<HTMLDivElement>) {
        const g = gesture.current;
        if (g?.pointer !== e.pointerId) return;
        g.clientX = e.clientX; g.clientY = e.clientY;
        update(g); scrollAtEdge(g);
      },
      onPointerUp(e: PointerEvent<HTMLDivElement>) {
        const g = gesture.current;
        if (g?.pointer !== e.pointerId) return;
        g.clientX = e.clientX; g.clientY = e.clientY;
        update(g); end(false);
      },
      onPointerCancel(e: PointerEvent<HTMLDivElement>) { if (gesture.current?.pointer === e.pointerId) end(true); },
      onLostPointerCapture(e: PointerEvent<HTMLDivElement>) { if (gesture.current?.pointer === e.pointerId) end(true); },
    },
    cancel: () => end(true),
  };
}

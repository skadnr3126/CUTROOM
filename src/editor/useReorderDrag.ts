import { useRef, useState, type DragEvent } from "react";

type Insertion = { targetId: string; after: boolean };

/** Keep internal moves separate from source-bin copies and external file drops. */
export function useReorderDrag(type: string, scope: string, ids: string[], busy: boolean,
  move: (id: string, targetId: string, after: boolean) => void, axis: "x" | "y" = "x") {
  const drag = useRef<{ scope: string; id: string } | null>(null);
  const [marker, setMarker] = useState<Insertion | null>(null);

  function clear() {
    drag.current = null;
    setMarker(null);
  }

  function insertion(event: DragEvent<HTMLElement>, targetId: string) {
    const source = drag.current;
    if (busy || !source || source.scope !== scope || source.id === targetId || !event.dataTransfer.types.includes(type)) return null;
    const from = ids.indexOf(source.id);
    const target = ids.indexOf(targetId);
    if (from < 0 || target < 0) return null;
    const rect = event.currentTarget.getBoundingClientRect();
    const position = axis === "x" ? (event.clientX - rect.left) / rect.width : (event.clientY - rect.top) / rect.height;
    const after = Math.abs(position - 0.5) < 0.05 ? from < target : position >= 0.5;
    return { targetId, after };
  }

  return (id: string) => ({
    draggable: !busy && ids.length > 1,
    "data-insert": marker?.targetId === id ? (marker.after ? "after" : "before") : undefined,
    onDragStart(event: DragEvent<HTMLElement>) {
      if (busy || ids.length < 2) { event.preventDefault(); return; }
      drag.current = { scope, id };
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData(type, id);
    },
    onDragEnd: clear,
    onDragOverCapture(event: DragEvent<HTMLElement>) {
      const next = insertion(event, id);
      if (!next) return;
      event.preventDefault();
      event.stopPropagation();
      event.dataTransfer.dropEffect = "move";
      if (marker?.targetId !== id || marker.after !== next.after) setMarker(next);
    },
    onDragLeaveCapture(event: DragEvent<HTMLElement>) {
      if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
      setMarker(current => current?.targetId === id ? null : current);
    },
    onDropCapture(event: DragEvent<HTMLElement>) {
      const next = insertion(event, id);
      if (!next) return;
      event.preventDefault();
      event.stopPropagation();
      const sourceId = event.dataTransfer.getData(type);
      if (sourceId === drag.current?.id) move(sourceId, id, next.after);
      clear();
    },
  });
}

import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent } from "react";
import type { RecordingRegion, SelectRegion } from "./recordingRegion";

type Request = { snapshot: ImageBitmap; finish: (rect: RecordingRegion | null) => void };
type Point = { x: number; y: number };

export function useRecordingRegionPicker() {
  const [request, setRequest] = useState<Request | null>(null);
  const choose: SelectRegion = (snapshot, signal) => new Promise((resolve, reject) => {
    function cancel() { finish(null); }
    function finish(rect: RecordingRegion | null) {
      signal.removeEventListener("abort", cancel);
      setRequest(null);
      if (rect) resolve(rect);
      else reject(new DOMException("영역 녹화를 취소했습니다.", "AbortError"));
    }
    if (signal.aborted) { cancel(); return; }
    signal.addEventListener("abort", cancel, { once: true });
    setRequest({ snapshot, finish });
  });
  return { choose, picker: request && <RegionDialog {...request} /> };
}

function RegionDialog({ snapshot, finish }: Request) {
  const dialog = useRef<HTMLDialogElement>(null);
  const preview = useRef<HTMLCanvasElement>(null);
  const anchor = useRef<{ id: number; point: Point } | null>(null);
  const [rect, setRect] = useState<RecordingRegion | null>(null);
  const [dragging, setDragging] = useState(false);
  useLayoutEffect(() => {
    const el = dialog.current!;
    preview.current!.getContext("2d")!.drawImage(snapshot, 0, 0);
    el.showModal();
    return () => el.close();
  }, [snapshot]);
  useEffect(() => { dialog.current?.querySelector("button")?.focus(); }, []);

  function point(e: PointerEvent) {
    const bounds = e.currentTarget.getBoundingClientRect();
    const pixel = (v: number, size: number) => Math.max(0, Math.min(size, Math.round(v * size)));
    return { x: pixel((e.clientX - bounds.left) / bounds.width, snapshot.width),
      y: pixel((e.clientY - bounds.top) / bounds.height, snapshot.height) };
  }
  function update(e: PointerEvent) {
    if (anchor.current?.id !== e.pointerId) return;
    const a = anchor.current.point, b = point(e);
    setRect({ x: Math.min(a.x,b.x), y: Math.min(a.y,b.y), width: Math.abs(a.x-b.x), height: Math.abs(a.y-b.y) });
  }
  function cancelDrag(e: PointerEvent) {
    if (anchor.current?.id !== e.pointerId) return;
    anchor.current = null; setDragging(false); setRect(null);
  }
  const valid = !!rect && rect.width >= 2 && rect.height >= 2;
  return <dialog ref={dialog} className="region-dialog" aria-labelledby="record-region-title"
    onCancel={e => { e.preventDefault(); finish(null); }}>
    <header><h2 id="record-region-title">녹화 영역 지정</h2><span>녹화할 부분을 드래그하세요.</span></header>
    <div className="region-surface" style={{ aspectRatio: snapshot.width + " / " + snapshot.height,
      width: `min(100%, ${snapshot.width}px, ${62 * snapshot.width / snapshot.height}vh)` }}
      onPointerDown={e => {
        if (e.button !== 0 || anchor.current) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        anchor.current = { id: e.pointerId, point: point(e) }; setDragging(true); setRect(null);
      }} onPointerMove={update} onPointerUp={e => {
        if (anchor.current?.id !== e.pointerId) return;
        update(e); anchor.current = null; setDragging(false);
      }} onPointerCancel={cancelDrag} onLostPointerCapture={cancelDrag}>
      <canvas ref={preview} width={snapshot.width} height={snapshot.height} aria-label="공유 화면 미리보기" />
      {rect && <span className="record-region-box" style={{ left: rect.x / snapshot.width * 100 + "%",
        top: rect.y / snapshot.height * 100 + "%", width: rect.width / snapshot.width * 100 + "%",
        height: rect.height / snapshot.height * 100 + "%" }} />}
    </div>
    <footer><output>{valid && `${rect.width} × ${rect.height} px`}</output>
      <button type="button" className="button compact" onClick={() => finish(null)}>취소</button>
      <button type="button" className="button primary" disabled={!valid || dragging} onClick={() => finish(rect)}>이 영역 녹화</button>
    </footer>
  </dialog>;
}

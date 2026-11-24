export type RecordingRegion = { x: number; y: number; width: number; height: number };
export type SelectRegion = (snapshot: ImageBitmap, signal: AbortSignal) => Promise<RecordingRegion>;

type ChromiumVideoStreams = {
  MediaStreamTrackProcessor?: new (init: { track: MediaStreamTrack; maxBufferSize: number }) => {
    readable: ReadableStream<VideoFrame>;
  };
  MediaStreamTrackGenerator?: new (init: { kind: "video" }) => MediaStreamTrack & {
    writable: WritableStream<VideoFrame>;
  };
};

export async function prepareRegionTrack(source: MediaStreamTrack, signal: AbortSignal, select: SelectRegion) {
  const api = window as Window & ChromiumVideoStreams;
  if (!api.MediaStreamTrackProcessor || !api.MediaStreamTrackGenerator)
    throw new Error("영역 녹화에 필요한 기능이 없습니다. WebView2를 업데이트해 주세요.");

  const reader = new api.MediaStreamTrackProcessor({ track: source, maxBufferSize: 1 }).readable.getReader();
  const output = new api.MediaStreamTrackGenerator({ kind: "video" });
  const writer = output.writable.getWriter();
  const lifetime = new AbortController();
  function close() {
    if (lifetime.signal.aborted) return;
    lifetime.abort();
    signal.removeEventListener("abort", close);
    source.removeEventListener("ended", close);
    void reader.cancel().catch(() => {});
    void writer.abort().catch(() => {});
    output.stop();
  }
  signal.addEventListener("abort", close, { once: true });
  source.addEventListener("ended", close, { once: true });
  let snapshot: ImageBitmap | undefined;
  try {
    if (signal.aborted || source.readyState === "ended") throw new Error("화면 공유가 종료되었습니다.");
    const first = await reader.read();
    if (first.done) throw new Error("녹화할 화면을 읽을 수 없습니다.");
    const originalWidth = first.value.displayWidth;
    const originalHeight = first.value.displayHeight;
    const firstTimestamp = first.value.timestamp;
    const firstArrival = performance.now();
    try { snapshot = await createImageBitmap(first.value); }
    finally { first.value.close(); }
    const region = await select(snapshot, lifetime.signal);
    lifetime.signal.throwIfAborted();
    // Draw only the selected pixels into encoder-compatible output dimensions.
    const width = region.width + region.width % 2;
    const height = region.height + region.height % 2;
    const canvas = new OffscreenCanvas(width, height);
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) throw new Error("녹화 영역을 준비할 수 없습니다.");
    const ctx = context;
    ctx.drawImage(snapshot, region.x, region.y, region.width, region.height, 0, 0, region.width, region.height);

    let lastWrite = 0;
    async function emit() {
      lastWrite = performance.now();
      const timestamp = Math.round(firstTimestamp + (lastWrite - firstArrival) * 1000);
      const cropped = new VideoFrame(canvas, { timestamp });
      try { await writer.write(cropped); } finally { cropped.close(); }
    }
    async function pump() {
      // Some capture sources deliver frames only when pixels change. Keep static
      // selections timed in the MP4, without waiting for an animation frame.
      const heartbeat = window.setInterval(() => {
        if (!lifetime.signal.aborted && performance.now() - lastWrite >= 450 && (writer.desiredSize ?? 0) > 0)
          void emit().catch(close);
      }, 500);
      try {
        while (!lifetime.signal.aborted) {
          const next = await reader.read();
          if (next.done) return;
          const frame = next.value;
          try {
            const scaleX = frame.displayWidth / originalWidth;
            const scaleY = frame.displayHeight / originalHeight;
            ctx.drawImage(frame, region.x * scaleX, region.y * scaleY,
              region.width * scaleX, region.height * scaleY, 0, 0, region.width, region.height);
          } finally { frame.close(); }
          await emit();
        }
      } finally { clearInterval(heartbeat); }
    }
    return { track: output, pump, close };
  } catch (error) { close(); throw error; }
  finally { snapshot?.close(); }
}

import { useLayoutEffect, useRef, useState } from "react";

export type PreviewVideoSource = {
  url: string;
  // Absolute timeline time corresponding to source time zero.
  originMs: number;
  timeMs: number;
  endMs?: number;
  muted?: boolean;
  volume?: number;
  onDuration?: (durationMs: number) => void;
};

function decoderKey(source: PreviewVideoSource) {
  return JSON.stringify([source.url, source.originMs]);
}

type Slot = "current" | "preroll" | "held";

// The native video clock can lead the timeline clock. Close to an exclusive
// outpoint, keep displaying the last frame whose presentation time is in range.
function guardOutpoint(video: HTMLVideoElement, canvas: HTMLCanvasElement, endMs: number) {
  const context = canvas.getContext("2d");
  if (!context) return;

  function draw(seconds: number) {
    if (seconds * 1000 >= endMs - 0.001 || video.seeking ||
        video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA || !video.videoWidth) return;

    const displayWidth = (video.clientWidth || video.videoWidth) * window.devicePixelRatio;
    const scale = Math.min(1, displayWidth / video.videoWidth);
    const width = Math.max(1, Math.round(video.videoWidth * scale));
    const height = Math.max(1, Math.round(video.videoHeight * scale));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    context!.drawImage(video, 0, 0, width, height);
    canvas.hidden = false;
    video.style.opacity = "0";
  }

  draw(video.currentTime);
  const frame: VideoFrameRequestCallback = (_, metadata) => {
    draw(metadata.mediaTime);
    callback = video.requestVideoFrameCallback(frame);
  };
  let callback = video.requestVideoFrameCallback(frame);
  return () => video.cancelVideoFrameCallback(callback);
}

function VideoSlot({ source, slot, visible, playing, rate, seekVersion, guardEnd, ready, fail }: {
  source: PreviewVideoSource;
  slot: Slot;
  visible: boolean;
  playing: boolean;
  rate: number;
  seekVersion: number;
  guardEnd?: number;
  ready: () => void;
  fail: () => void;
}) {
  const ref = useRef<HTMLVideoElement>(null);
  const surface = useRef<HTMLCanvasElement>(null);
  const appliedSeek = useRef<number | undefined>(undefined);
  const reportedDuration = useRef<number | undefined>(undefined);

  useLayoutEffect(() => {
    const video = ref.current;
    const canvas = surface.current;
    if (!video || !canvas || slot === "held") return;
    if (guardEnd !== undefined) return guardOutpoint(video, canvas, guardEnd);
    video.style.opacity = "";
    canvas.hidden = true;
  }, [guardEnd, slot]);

  function sync() {
    const video = ref.current;
    if (!video) return;
    const volume = source.volume ?? 1;
    if (video.volume !== volume) video.volume = volume;
    if (slot === "held") {
      if (!video.paused) video.pause();
      return;
    }
    if (video.error) {
      if (slot === "current") fail();
      return;
    }
    if (video.readyState < HTMLMediaElement.HAVE_METADATA) return;
    const durationMs = Math.round(video.duration * 1000);
    if (source.onDuration && Number.isFinite(durationMs) && durationMs > 0 && reportedDuration.current !== durationMs) {
      reportedDuration.current = durationMs;
      source.onDuration(durationMs);
    }
    if (video.playbackRate !== rate) video.playbackRate = rate;
    const seconds = source.timeMs / 1000;
    const end = Number.isFinite(video.duration) ? Math.max(0, video.duration - 0.001) : Infinity;
    const target = Math.min(seconds, end);
    const play = slot === "current" && playing && seconds < end;
    // An explicit jump must land even if it falls inside the normal drift tolerance.
    const seekChanged = appliedSeek.current !== seekVersion;
    const tolerance = seekChanged ? 0.001 : play ? 0.28 : 0.016;
    if ((seekChanged || !video.seeking) && Math.abs(video.currentTime - target) > tolerance) video.currentTime = target;
    appliedSeek.current = seekVersion;
    if (slot === "current" && !video.seeking && video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) ready();
    if (play && video.paused) void video.play().catch(() => {});
    else if (!play && !video.paused) video.pause();
  }
  useLayoutEffect(sync);

  const visibility = visible ? "visible" : "hidden";
  return <>
    <video ref={ref} src={source.url} playsInline preload="auto" muted={slot !== "current" || !!source.muted}
      style={{ visibility }} aria-hidden={!visible} onLoadedMetadata={sync} onLoadedData={sync}
      onCanPlay={sync} onSeeked={sync} onError={sync} />
    <canvas ref={surface} hidden aria-hidden="true" className="cut-preview-frame" style={{ visibility }} />
  </>;
}

export function SeamlessVideo({ current, next, playing, rate, seekVersion }: {
  current: PreviewVideoSource; next?: PreviewVideoSource; playing: boolean; rate: number; seekVersion: number;
}) {
  const [presented, setPresented] = useState<PreviewVideoSource>();
  const [failed, setFailed] = useState<string>();
  const key = decoderKey(current);
  const continuous = next && decoderKey(next) === key;
  const guardEnd = playing && !continuous && current.endMs !== undefined &&
    current.endMs - current.timeMs <= 350 * rate ? current.endMs : undefined;
  const held = presented && decoderKey(presented) !== key ? presented : undefined;
  const slots: { source: PreviewVideoSource; slot: Slot }[] = [];
  if (held) slots.push({ source: held, slot: "held" });
  slots.push({ source: current, slot: "current" });
  if (next && !slots.some(entry => decoderKey(entry.source) === decoderKey(next))) {
    slots.push({ source: next, slot: "preroll" });
  }

  function ready() {
    if (!presented || held) setPresented(current);
    if (failed) setFailed(undefined);
  }
  function fail() {
    if (failed !== key) setFailed(key);
  }
  return <>
    {slots.map(({ source, slot }) => <VideoSlot key={decoderKey(source)} source={source} slot={slot}
      rate={rate} playing={playing} seekVersion={seekVersion} guardEnd={slot === "current" ? guardEnd : undefined}
      visible={failed !== key && (slot === "held" || (slot === "current" && !held))} ready={ready} fail={fail} />)}
    {failed === key && <p className="preview-error">이 영상 소스를 재생할 수 없습니다.</p>}
  </>;
}

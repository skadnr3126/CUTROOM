import { useLayoutEffect, useRef, useState } from "react";

export function useTimelinePlayback(active: boolean, endMs: number, rate: number) {
  const [position, setPosition] = useState(0);
  const [playing, setPlaying] = useState(false);
  const positionRef = useRef(0);
  const seekVersion = useRef(0);
  const lastTick = useRef(0);

  function move(ms: number) {
    positionRef.current = ms;
    setPosition(ms);
  }

  function seek(ms: number) {
    seekVersion.current++;
    lastTick.current = performance.now();
    move(ms);
  }

  useLayoutEffect(() => {
    if (!playing || !active) return;
    lastTick.current = performance.now();
    let frame = requestAnimationFrame(tick);
    function tick() {
      const now = performance.now();
      const next = Math.min(endMs, positionRef.current + (now - lastTick.current) * rate);
      lastTick.current = now;
      move(next);
      if (next < endMs) frame = requestAnimationFrame(tick);
      else setPlaying(false);
    }
    return () => cancelAnimationFrame(frame);
  }, [playing, active, endMs, rate]);

  return { position, positionRef, seek, seekVersion, playing, setPlaying };
}

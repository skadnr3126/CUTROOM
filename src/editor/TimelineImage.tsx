import { useLayoutEffect, useRef, useState } from "react";
import type { ClipFrame } from "./project";
import { frameStyle } from "./visualTracks";

function loadImage(url: string) {
  const image = new Image();
  image.src = url;
  const entry = { image, decoded: false, ready: image.decode().then(
    () => { entry.decoded = true; },
    () => { /* Report failures only if this image is still requested on the timeline. */ },
  ) };
  return entry;
}

type Presentation = { image?: HTMLImageElement; frame?: ClipFrame };
type Props = {
  url: string; name: string; frame?: ClipFrame; nextUrl?: string;
  startMs: number; endMs: number; playing: boolean; seekVersion: number;
};

export function TimelineImage({ url, name, frame, nextUrl, startMs, endMs, playing, seekVersion }: Props) {
  const cache = useRef(new Map<string, ReturnType<typeof loadImage>>());
  const surface = useRef<HTMLSpanElement>(null);
  const lastTarget = useRef<{ url: string; startMs: number; endMs: number; seekVersion: number }>(undefined);
  const [shown, setShown] = useState<Presentation>();

  useLayoutEffect(() => {
    const entries = cache.current;
    return () => {
      for (const entry of entries.values()) if (!entry.decoded) entry.image.removeAttribute("src");
      entries.clear();
    };
  }, []);

  useLayoutEffect(() => {
    for (const [key, entry] of cache.current) {
      if (key !== url && key !== nextUrl) {
        if (!entry.decoded) entry.image.removeAttribute("src");
        cache.current.delete(key);
      }
    }
    for (const key of [url, nextUrl]) {
      if (key && !cache.current.has(key)) cache.current.set(key, loadImage(key));
    }
  }, [url, nextUrl]);

  useLayoutEffect(() => {
    const previous = lastTarget.current;
    lastTarget.current = { url, startMs, endMs, seekVersion };
    const samePlacement = previous?.url === url && previous.startMs === startMs;
    const continuous = playing && previous?.seekVersion === seekVersion && (samePlacement || previous.endMs === startMs);
    const entry = cache.current.get(url)!;
    if (!continuous && !entry.decoded) setShown(undefined);
    let active = true;
    const present = () => {
      if (!active) return;
      entry.image.alt = name;
      setShown({ image: entry.decoded ? entry.image : undefined, frame });
    };
    if (entry.decoded) present();
    else void entry.ready.then(present);
    return () => { active = false; };
  }, [url, name, startMs, endMs, playing, seekVersion, frame?.size, frame?.x, frame?.y]);

  // Mount the exact node that finished decoding, with its own placement. Replacing
  // src on the visible node could expose the backing layer while Chromium decodes.
  useLayoutEffect(() => {
    const host = surface.current;
    if (shown?.image) host?.replaceChildren(shown.image);
    else host?.replaceChildren();
  }, [shown?.image]);

  return <div className="preview-layer" style={frameStyle(shown?.frame)}>
    <span ref={surface} />
    {shown && !shown.image && <p className="preview-error">이미지를 열 수 없습니다.</p>}
  </div>;
}

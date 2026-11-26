import { useEffect, useRef, useState } from "react";
import type { ClipVolumeTarget, Scene } from "./project";

export function useClipVolume(scene: Scene | null, clipId: string | null | undefined, saved: number,
  persist: (scene: Scene, target: ClipVolumeTarget, value: number) => Promise<unknown>) {
  // null selects the scene's dedicated audio. undefined means no audible clip is selected.
  const key = scene && clipId !== undefined ? JSON.stringify([scene.id, scene.revision, clipId]) : null;
  const target: ClipVolumeTarget | null = !scene || clipId === undefined ? null :
    clipId === null ? { kind: "sceneAudio" } : { kind: "clip", clipId };
  const [draft, setDraft] = useState<{ key: string; value: number } | null>(null);
  const value = draft?.key === key ? draft.value : undefined;

  useEffect(() => { setDraft(previous => previous?.key === key ? previous : null); }, [key]);

  function previewGain(owner: Scene, id: string | null, volume?: number) {
    const audition = owner.id === scene?.id && id === clipId ? value : undefined;
    return (audition ?? volume ?? 100) / 100;
  }
  function preview(percent: number | null) {
    if (key && percent !== null) setDraft({ key, value: percent });
    else setDraft(previous => previous?.key === key ? null : previous);
  }
  async function save(percent: number) {
    if (!scene || !target) return;
    if (percent === saved) { preview(null); return; }
    preview(percent);
    try {
      await persist(scene, target, percent);
    } finally {
      preview(null);
    }
  }
  return { key, target, value: value ?? saved, previewGain, preview, save };
}

type Props = {
  version: string;
  value: number;
  disabled: boolean;
  preview: (value: number | null) => void;
  save: (value: number) => void;
};

const sliderKeys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"];

export function ClipVolumeControl({ version, value, disabled, preview, save }: Props) {
  const adjusting = useRef<string | null>(null);
  const [typed, setTyped] = useState<{ version: string; text: string } | null>(null);

  function finish(input: HTMLInputElement) {
    const started = adjusting.current;
    adjusting.current = null;
    if (started !== version) return;
    if (disabled) preview(null);
    else save(input.valueAsNumber);
  }

  return <div className="clip-volume" role="group" aria-label="클립 음량 조절">
    <label>음량 <input type="range" aria-label="클립 음량" aria-valuetext={value + "%"}
      min={0} max={100} step={1} value={value} aria-disabled={disabled}
      onPointerDown={e => {
        if (disabled) { e.preventDefault(); return; }
        adjusting.current = version;
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onKeyDown={e => {
        if (!sliderKeys.includes(e.key)) return;
        if (disabled) e.preventDefault();
        else adjusting.current ??= version;
      }}
      onChange={e => {
        if (disabled) return;
        if (adjusting.current === version) preview(e.currentTarget.valueAsNumber);
        else if (adjusting.current === null) save(e.currentTarget.valueAsNumber);
      }}
      onPointerUp={e => finish(e.currentTarget)} onKeyUp={e => finish(e.currentTarget)} onBlur={e => finish(e.currentTarget)}
      onPointerCancel={() => { adjusting.current = null; preview(null); }} /></label>
    <label><input type="number" aria-label="클립 음량 백분율" min={0} max={100} step={1}
      value={typed?.version === version ? typed.text : value} disabled={disabled} onChange={e => setTyped({ version, text: e.currentTarget.value })}
      onKeyDown={e => { if (e.key === "Enter") e.currentTarget.blur(); }}
      onBlur={e => {
        if (!disabled && typed?.version === version && e.currentTarget.validity.valid && Number.isInteger(e.currentTarget.valueAsNumber)) {
          save(e.currentTarget.valueAsNumber);
        }
        setTyped(null);
      }} />%</label>
  </div>;
}

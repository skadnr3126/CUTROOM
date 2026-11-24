import { useLayoutEffect, useRef, useState } from "react";
import type { Scene } from "./project";

type Surface = "script" | "timeline";
type Rename = { id: string; surface: Surface; original: string; value: string };

export function useSceneRename(save: (session: Rename) => Promise<boolean>) {
  const [active, setActive] = useState<Rename | null>(null);
  return {
    active,
    start(scene: Scene, surface: Surface) {
      const original = scene.name ?? "";
      setActive({ id: scene.id, surface, original, value: original });
    },
    change(value: string) {
      setActive(current => current && { ...current, value });
    },
    cancel() { setActive(null); },
    async commit() {
      if (!active) return false;
      if (active.value.trim() !== active.original && !await save(active)) return false;
      setActive(current => current === active ? null : current);
      return true;
    },
  };
}

export type SceneRename = ReturnType<typeof useSceneRename>;

export function SceneTitleInput({ rename, fallback, busy, saveOnBlur = false }: { rename: SceneRename; fallback: string; busy: boolean; saveOnBlur?: boolean }) {
  const input = useRef<HTMLInputElement>(null);
  const finishing = useRef(false);
  useLayoutEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  async function commit() {
    if (busy || finishing.current) return false;
    finishing.current = true;
    const saved = await rename.commit();
    if (!saved) finishing.current = false;
    return saved;
  }

  return <input ref={input} className="scene-title-input" aria-label="구간 이름" placeholder={fallback}
    value={rename.active?.value ?? ""} readOnly={busy} onChange={event => rename.change(event.target.value)}
    onBlur={() => {
      if (finishing.current) return;
      if (saveOnBlur) void commit();
      else rename.cancel();
    }}
    onKeyDown={async event => {
      event.stopPropagation();
      if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229 ||
          (event.key !== "Enter" && event.key !== "Escape")) return;
      event.preventDefault();
      if (event.repeat || finishing.current) return;
      const row = event.currentTarget.parentElement;
      if (event.key === "Escape") {
        finishing.current = true;
        rename.cancel();
      } else if (!await commit()) return;
      requestAnimationFrame(() => {
        if (document.activeElement === document.body) row?.querySelector<HTMLButtonElement>("[data-scene-title]")?.focus();
      });
    }} />;
}

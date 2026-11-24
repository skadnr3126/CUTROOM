import { useEffect, useLayoutEffect, useRef } from "react";

type Action = { label: string; run: () => void };
export type MenuAction = { x: number; y: number; items: Action[] };

export function ClipMenu({ action, close }: { action: MenuAction; close: () => void }) {
  const button = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const node = button.current;
    if (!node) return;
    const rect = node.getBoundingClientRect();
    node.style.left = Math.max(0, Math.min(action.x, innerWidth - rect.width - 4)) + "px";
    node.style.top = Math.max(0, Math.min(action.y, innerHeight - rect.height - 4)) + "px";
    node.querySelector("button")?.focus({ preventScroll: true });
  }, [action]);
  useEffect(() => {
    const outside = (event: PointerEvent) => { if (!button.current?.contains(event.target as Node)) close(); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape);
    window.addEventListener("blur", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape);
      window.removeEventListener("blur", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [close]);
  return <div role="menu" ref={button} className="clip-menu-action">{action.items.map((item) => <button key={item.label} role="menuitem" onClick={() => {
    close();
    item.run();
  }}>{item.label}</button>)}</div>;
}

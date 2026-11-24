import { useEffect } from "react";

export function useTimelineUndoKey(undo: () => void) {
  useEffect(() => {
    function handle(event: KeyboardEvent) {
      if (event.defaultPrevented || event.isComposing || !event.ctrlKey || event.altKey || event.shiftKey || event.metaKey ||
        (event.code !== "KeyZ" && event.key.toLowerCase() !== "z")) return;
      const target = event.target;
      if (target instanceof HTMLElement && (target.isContentEditable ||
        target.closest('dialog, textarea, select, input:not([type="range"]), [role="textbox"], [role="dialog"]'))) return;
      event.preventDefault();
      if (!event.repeat) undo();
    }
    window.addEventListener("keydown", handle);
    return () => window.removeEventListener("keydown", handle);
  }, [undo]);
}

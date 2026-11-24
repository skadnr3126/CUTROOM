import { isTauri } from "@tauri-apps/api/core";
import { invoke } from "./projectSession";
import { binSelectionType } from "./binDrag";
import { sourceDragType } from "./project";

export function supportsFileDrag() {
  return isTauri() && navigator.userAgent.includes("Windows");
}

export function writeBinDrag(data: DataTransfer, primary: string, ids: string[]) {
  data.setData(sourceDragType, primary);
  data.setData(binSelectionType, JSON.stringify(ids));
  data.effectAllowed = "copyMove";
}

// Explorer receives real files. Inside this WebView the same gesture should use
// the original project IDs, including when the pointer leaves and comes back.
function routeBinFileDrag(primary: string, ids: string[]) {
  const events = ["dragenter", "dragover", "dragleave", "drop"] as const;
  const data = new DataTransfer();
  writeBinDrag(data, primary, ids);
  let native: DataTransfer | null = null;
  Object.defineProperty(data, "dropEffect", {
    get: () => native?.dropEffect ?? "none",
    set: (effect: DataTransfer["dropEffect"]) => {
      if (native) native.dropEffect = effect === "none" ? "none" : "copy";
    },
  });
  let overTarget = false;
  let dropped: (() => void) | undefined;
  function route(event: DragEvent) {
    if (!event.dataTransfer?.types.includes("Files")) return;
    native = event.dataTransfer;
    overTarget = event.type === "dragenter" || event.type === "dragover";
    if (overTarget) native.dropEffect = "copy";
    Object.defineProperty(event, "dataTransfer", { value: data });
    if (event.type === "drop") {
      event.preventDefault();
      dropped?.();
    }
  }
  for (const type of events) window.addEventListener(type, route, true);
  return async (copied: boolean) => {
    // WebView2 may deliver the renderer's drop just after the native drag loop returns.
    if (copied && overTarget) await new Promise<void>(resolve => {
      const timer = window.setTimeout(resolve, 300);
      dropped = () => { window.clearTimeout(timer); resolve(); };
    });
    for (const type of events) window.removeEventListener(type, route, true);
  };
}

let dragging = false;
export async function dragBinFiles(primary: string, ids: string[], start: () => void, end: () => void) {
  if (dragging) return;
  dragging = true;
  const stopRouting = routeBinFileDrag(primary, ids);
  let copied = false;
  try {
    start();
    copied = await invoke<boolean>("drag_bin_files", { ids });
  } finally {
    await stopRouting(copied);
    dragging = false;
    end();
  }
}

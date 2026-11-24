import { sourceDragType } from "./project";

export const binSelectionType = "application/x-cutroom-bin-selection";
export const folderDragType = "application/x-cutroom-folder";

export function draggedSourceIds(data: DataTransfer): string[] {
  const primary = data.getData(sourceDragType);
  if (!primary) return [];
  try {
    const group: unknown = JSON.parse(data.getData(binSelectionType) || "null");
    if (Array.isArray(group) && group.includes(primary) && group.every(id => typeof id === "string")) return group;
  } catch { /* Single-source drags only need the primary ID. */ }
  return [primary];
}

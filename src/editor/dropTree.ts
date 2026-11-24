// Snapshot the drag data while the browser still grants access in the drop event.
export type ExternalDrop = { items: (FileSystemEntry | File)[] };
export type MediaBatch = {
  folders: { name: string; parentIndex: number | null }[];
  files: { file: File; folderIndex: number | null }[];
  skipped: number;
};

export function captureExternalDrop(data: DataTransfer): ExternalDrop {
  const items: ExternalDrop["items"] = [];
  for (const item of Array.from(data.items)) {
    if (item.kind !== "file") continue;
    const entry = item.webkitGetAsEntry?.() ?? item.getAsFile();
    if (entry) items.push(entry);
  }
  return { items: items.length ? items : Array.from(data.files) };
}

export function flatBatch(files: File[]): MediaBatch {
  return { folders: [], files: files.map(file => ({ file, folderIndex: null })), skipped: 0 };
}

export async function readDropTree(drop: ExternalDrop, extensions: readonly string[]): Promise<MediaBatch> {
  const batch = flatBatch([]);
  const supported = new Set(extensions);
  const containsFolder = drop.items.some(item => "isDirectory" in item ? item.isDirectory : !!item.webkitRelativePath);
  function include(name: string) {
    const dot = name.lastIndexOf(".");
    if (!containsFolder || (dot > 0 && supported.has(name.slice(dot + 1).toLowerCase()))) return true;
    batch.skipped++;
    return false;
  }

  async function visit(entry: FileSystemEntry, parentIndex: number | null): Promise<void> {
    const failed = () => new Error(`가져올 항목을 읽을 수 없습니다. (${entry.fullPath || entry.name})`);
    if (entry.isFile) {
      if (!include(entry.name)) return;
      const file = await new Promise<File>((resolve, reject) =>
        (entry as FileSystemFileEntry).file(resolve, () => reject(failed())));
      batch.files.push({ file, folderIndex: parentIndex });
      return;
    }
    const folderIndex = batch.folders.push({ name: entry.name, parentIndex }) - 1;
    const reader = (entry as FileSystemDirectoryEntry).createReader();
    for (;;) {
      const entries = await new Promise<FileSystemEntry[]>((resolve, reject) =>
        reader.readEntries(resolve, () => reject(failed())));
      if (!entries.length) break;
      for (const child of entries) await visit(child, folderIndex);
    }
  }

  const relativeFolders = new Map<string, number>();
  for (const item of drop.items) {
    if ("isDirectory" in item) { await visit(item, null); continue; }
    let parentIndex: number | null = null;
    let path = "";
    for (const name of item.webkitRelativePath.split("/").slice(0, -1)) {
      path += "/" + name;
      let index = relativeFolders.get(path);
      if (index === undefined) {
        index = batch.folders.push({ name, parentIndex }) - 1;
        relativeFolders.set(path, index);
      }
      parentIndex = index;
    }
    if (include(item.name)) batch.files.push({ file: item, folderIndex: parentIndex });
  }
  return batch;
}

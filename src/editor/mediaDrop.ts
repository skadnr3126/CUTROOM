import { flatBatch, type MediaBatch } from "./dropTree";
import { invoke } from "./projectSession";

// Keep native file drops and the editor's internal HTML drag targets on the same DOM event path.
export async function importDroppedMedia<T>(input: File[] | MediaBatch, sourceJobId?: string, folderId: string | null = null): Promise<T> {
  const { folders, files } = Array.isArray(input) ? flatBatch(input) : input;
  const id = await invoke<string>("begin_media_drop", {
    folders,
    files: files.map(({ file, folderIndex }) => ({ name: file.name, size: file.size, folderIndex })),
  });
  const chunkSize = 2 * 1024 * 1024;
  try {
    for (const [index, { file }] of files.entries()) {
      for (let offset = 0; offset < file.size; offset += chunkSize) {
        const bytes = await file.slice(offset, offset + chunkSize).arrayBuffer().catch(() => {
          throw new Error(`파일을 읽을 수 없습니다. (${file.name})`);
        });
        await invoke("append_media_drop", bytes, { headers: {
          "x-drop-id": id, "x-file-index": String(index), "x-file-offset": String(offset),
        } });
      }
    }
  } catch (error) {
    await invoke("end_media_drop", { id, commit: false }).catch(() => {});
    throw error;
  }
  return invoke<T>("end_media_drop", { id, commit: true, sourceJobId, folderId });
}

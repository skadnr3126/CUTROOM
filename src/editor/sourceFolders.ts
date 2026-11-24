import type { SourceFolder } from "./project";

export function folderPath(id: string | null, byId: Map<string, SourceFolder>) {
  const path: SourceFolder[] = [];
  const seen = new Set<string>();
  let folder = id ? byId.get(id) : undefined;
  while (folder && !seen.has(folder.id)) {
    seen.add(folder.id);
    path.push(folder);
    folder = folder.parentId ? byId.get(folder.parentId) : undefined;
  }
  return path.reverse();
}

export function subtreeIds(folders: SourceFolder[], folder: string) {
  const children = new Map<string, string[]>();
  for (const f of folders) {
    if (!f.parentId) continue;
    const siblings = children.get(f.parentId);
    if (siblings) siblings.push(f.id);
    else children.set(f.parentId, [f.id]);
  }
  const descendants = new Set<string>();
  if (folders.some(f => f.id === folder)) descendants.add(folder);
  for (const id of descendants) {
    for (const child of children.get(id) ?? []) descendants.add(child);
  }
  return descendants;
}

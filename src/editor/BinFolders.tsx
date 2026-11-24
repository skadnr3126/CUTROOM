import { captureExternalDrop, type ExternalDrop } from "./dropTree";
import { useLayoutEffect, useMemo, useRef, useState, type DragEvent, type ReactNode } from "react";
import { sourceDragType, type FolderEdit, type SourceFolder } from "./project";
import type { MenuAction } from "./ClipMenu";

import { folderPath } from "./sourceFolders";
import { draggedSourceIds, folderDragType } from "./binDrag";
import { useFileMarquee } from "./useFileMarquee";
const rootName = "소스 보관함";

type Location = string | null;
export type FolderControls = {
  folders: SourceFolder[];
  active: Location;
  open: (id: Location) => void;
  edit: (action: FolderEdit) => Promise<boolean>;
  importFiles: (files: ExternalDrop, folderId: Location) => void;
};

export function DestinationPicker({ folders, exclude, busy, onMove, close }: {
  folders: SourceFolder[]; exclude?: string; busy: boolean; onMove: (id: Location) => void; close: () => void;
}) {
  const byId = new Map(folders.map(f => [f.id, f]));
  const options = folders.map(f => folderPath(f.id, byId)).filter(path => !path.some(f => f.id === exclude))
    .map(path => ({id: path[path.length-1].id, name: path.map(f => f.name).join(" › ")}))
    .sort((a,b) => a.name.localeCompare(b.name, "ko"));
  return <div className="folder-form">
    <label>이동할 위치 <select autoFocus aria-label="이동할 폴더" value="" disabled={busy}
      onChange={e => onMove(e.target.value === "root" ? null : e.target.value)}>
      <option value="" disabled>폴더 선택</option><option value="root">{rootName}</option>
      {options.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}
    </select></label><button className="text-button" onClick={close}>취소</button>
  </div>;
}

type Form = { kind: "rename" | "remove" | "move"; folder: SourceFolder };

export function BinFolders({ controls, busy, onMenu, move, mode, children, selected, select, empty, selectedSources, selectSources, finishSelection }: {
  controls: FolderControls; busy: boolean; onMenu: (menu: MenuAction) => void;
  move: (ids: string[], folderId: Location) => void; mode: "grid" | "list"; children: ReactNode;
  selectedSources: string[]; selectSources: (ids: string[]) => void; finishSelection: (ids: string[]) => void;
  selected: string | null; select: (id: string | null) => void; empty: boolean;
}) {
  const { folders, active, open, edit, importFiles } = controls;
  const byId = useMemo(() => new Map(folders.map(f => [f.id, f])), [folders]);
  const path = folderPath(active, byId);
  const parent = active ? byId.get(active)?.parentId ?? null : null;
  const entries = folders.filter(folder => (folder.parentId ?? null) === active)
    .sort((a,b) => a.name.localeCompare(b.name, "ko", {numeric:true}));
  const [form, setForm] = useState<Form | null>(null);
  const [name, setName] = useState("");
  const [hover, setHover] = useState<string>();
  const trail = useRef<HTMLElement>(null);
  const marquee = useFileMarquee(selectedSources, selectSources, finishSelection, busy);
  const listing = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (trail.current) trail.current.scrollLeft = trail.current.scrollWidth;
  }, [active, folders]);
  function openForm(value: Form) {
    setName(value.kind === "rename" ? value.folder.name : "");
    setForm(value);
  }
  async function createFolder() {
    if (busy) return;
    setForm(null);
    const siblingNames = new Set(entries.map(folder => folder.name));
    const base = "새 폴더";
    let folderName = base;
    for (let number = 2; siblingNames.has(folderName); number++) folderName = `${base} (${number})`;
    const id = crypto.randomUUID();
    if (!await edit({type:"createFolder", id, name:folderName, parentId:active})) return;
    select(id);
    requestAnimationFrame(() => listing.current?.querySelector<HTMLButtonElement>(`[data-folder-id="${id}"]`)?.focus());
  }
  async function save() {
    if (!form || busy) return;
    if (form.kind === "rename") {
      if (!await edit({type:"renameFolder", id:form.folder.id, name})) return;
    } else if (form.kind === "remove") {
      if (!await edit({type:"removeFolder", id:form.folder.id})) return;
      select(null);
    }
    setForm(null);
  }
  function folderMenu(folder: SourceFolder, x: number, y: number) {
    select(folder.id);
    onMenu({ x, y, items: [
      { label: "열기", run: () => open(folder.id) },
      { label: "이름 바꾸기", run: () => openForm({kind:"rename", folder}) },
      { label: "폴더로 이동", run: () => openForm({kind:"move", folder}) },
      { label: "폴더 삭제", run: () => openForm({kind:"remove", folder}) },
    ] });
  }

  function dropTarget(id: Location) {
    const key = id ?? "root";
    return {
      onDragOver(e: DragEvent) {
        const types = e.dataTransfer.types;
        if (busy || ![sourceDragType, folderDragType, "Files"].some(t => types.includes(t))) return;
        e.preventDefault(); e.stopPropagation();
        e.dataTransfer.dropEffect = types.includes("Files") ? "copy" : "move";
        setHover(key);
      },
      onDragLeave() { setHover(undefined); },
      onDrop(e: DragEvent) {
        e.preventDefault(); e.stopPropagation(); setHover(undefined);
        if (busy) return;
        if (e.dataTransfer.types.includes("Files")) { importFiles(captureExternalDrop(e.dataTransfer), id); return; }
        const dragged = e.dataTransfer.getData(folderDragType);
        if (dragged) {
          if (id !== dragged) void edit({type:"moveFolder", id:dragged, parentId:id});
          return;
        }
        const ids = draggedSourceIds(e.dataTransfer);
        if (ids.length) move(ids, id);
      },
      "data-drop-target": hover === key || undefined,
    };
  }

  return <>
    <div className="folder-bar">
      <button className="button compact" aria-label="상위 폴더로" title="상위 폴더로" disabled={!active} onClick={() => open(parent)}>↑</button>
      <nav className="folder-path" ref={trail} aria-label="현재 폴더 경로">
        {[{id:null, name:rootName}, ...path].map(f => <button key={f.id ?? "root"} aria-current={active === f.id ? "location" : undefined}
          title={f.name} onClick={() => open(f.id)} {...dropTarget(f.id)}>{f.name}</button>)}
      </nav>
      <button className="button compact" disabled={busy} onClick={createFolder}>새 폴더</button>
    </div>
    {form?.kind === "move" ? <DestinationPicker folders={folders} exclude={form.folder.id} busy={busy} close={() => setForm(null)}
      onMove={async id => { if (await edit({type:"moveFolder", id:form.folder.id, parentId:id})) setForm(null); }}/> :
      form && <form className="folder-form" onSubmit={e => { e.preventDefault(); void save(); }}
        onKeyDown={e => { if (e.key === "Escape") setForm(null); }}>
        {form.kind === "remove" ? <span>‘{form.folder.name}’ 폴더를 삭제할까요? 안의 폴더와 소스는 한 단계 위로 이동합니다.</span> :
          <input key={form.kind} aria-label="폴더 이름" placeholder="폴더 이름" autoFocus value={name}
            onFocus={e => e.currentTarget.select()} onChange={e => setName(e.target.value)} />}
        <button className="button compact" disabled={busy || (form.kind !== "remove" && !name.trim())}>
          {form.kind === "remove" ? "폴더 삭제" : "저장"}</button>
        <button className="text-button" type="button" onClick={() => setForm(null)}>취소</button>
      </form>}
    <div className={`source-list bin-${mode}`} ref={listing} {...marquee.events} tabIndex={0} aria-label="폴더 내용"
      onContextMenu={e => {
        if ((e.target as Element).closest("button")) return;
        e.preventDefault();
        if (!busy) onMenu({x:e.clientX,y:e.clientY,items:[{label:"새 폴더", run:createFolder}]});
      }} onKeyDown={e => {
        if (e.key === "Escape") marquee.cancel();
        if (active && e.altKey && e.key === "ArrowUp") { e.preventDefault(); open(parent); }
      }}>
      {entries.map(f => <button type="button" key={f.id} data-folder-id={f.id} className={"source-item folder-card" + (selected === f.id ? " selected" : "")}
        aria-label={"폴더 · "+f.name} aria-pressed={selected === f.id} title={f.name} onClick={() => select(f.id)} onDoubleClick={() => open(f.id)}
        onKeyDown={e => {
          if (e.key === "Enter") { e.preventDefault(); open(f.id); }
          if (!busy && e.key === "F2") { e.preventDefault(); openForm({kind:"rename",folder:f}); }
          if (!busy && e.key === "Delete" && !e.repeat) { e.preventDefault(); openForm({kind:"remove",folder:f}); }
        }} onContextMenu={e => {e.preventDefault(); if (!busy) folderMenu(f,e.clientX,e.clientY);}}
        draggable={!busy} onDragStart={e => {e.dataTransfer.setData(folderDragType,f.id); e.dataTransfer.effectAllowed="copyMove";}}
        onDragEnd={() => setHover(undefined)} {...dropTarget(f.id)}>
        <span className="folder-thumb" aria-hidden="true"><i /></span><span className="bin-name">{f.name}</span>
      </button>)}
      {children}
      {marquee.rectangle && <div className="bin-marquee" aria-hidden="true" style={marquee.rectangle}/>}
      {empty && !entries.length && <div className="center-empty"><strong>이 폴더가 비어 있습니다</strong>
        <p>파일을 끌어 놓거나 우클릭해 새 폴더를 만드세요.</p></div>}
    </div>
  </>;
}

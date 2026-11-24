import { dragBinFiles, supportsFileDrag, writeBinDrag } from "./binFileDrag";
import { useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { assetLabels, type Asset } from "./project";
import { BinThumbnail, type BinMode } from "./BinThumbnail";
import { SourceRename } from "./SourceRename";
import type { MenuAction } from "./ClipMenu";

import { BinFolders, DestinationPicker, type FolderControls } from "./BinFolders";

type Props = {
  folderControls: FolderControls;
  mediaDir: string; binAssets: Asset[]; source: Asset | null; captureControls: ReactNode;
  hasScene: boolean; busy: boolean;
  onImport: () => void; onLink: () => void; onChooseSource: (id: string) => void;
  onMenu: (menu: MenuAction) => void; onStartDrag: (id: string) => void; onEndDrag: () => void;
  onRenameSource: (id: string, name: string) => Promise<boolean>;
  onRelinkSource: (id: string) => void;
  onRemoveSource: (id: string) => void;
};

export function SourceBin({ folderControls, mediaDir, binAssets, source, captureControls,
  hasScene, busy, onImport, onLink, onMenu, onStartDrag, onEndDrag, onChooseSource, onRenameSource, onRelinkSource, onRemoveSource }: Props) {
  const { folders, active, edit } = folderControls;
  const visible = useMemo(() => binAssets.filter(asset => (asset.folderId ?? null) === active), [active, binAssets]);
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);
  const [dragError, setDragError] = useState("");
  const [batch, setBatch] = useState<string[] | null>(null);
  const [moveIds, setMoveIds] = useState<string[] | null>(null);
  const anchor = useRef<string | null>(null);
  const selection = useMemo(() => new Set(batch ?? (source ? [source.id] : [])), [batch, source]);
  const selected = useMemo(() => visible.filter(asset => selection.has(asset.id)).map(asset => asset.id), [selection, visible]);
  useEffect(() => {
    setSelectedFolder(null);
    anchor.current = null;
    setBatch(null);
    setMoveIds(null);
    setRenaming(null);
  }, [active]);
  function selectFiles(ids: string[]) {
    setSelectedFolder(null);
    setMoveIds(null);
    setRenaming(null);
    setBatch(ids);
  }
  function selectAsset(event: MouseEvent, id: string) {
    setSelectedFolder(null);
    setMoveIds(null);
    const index = visible.findIndex(asset => asset.id === anchor.current);
    if (event.shiftKey && index >= 0) {
      const end = visible.findIndex(asset => asset.id === id);
      setBatch(visible.slice(Math.min(index, end), Math.max(index, end) + 1).map(asset => asset.id));
    } else if (event.ctrlKey || event.metaKey) {
      setBatch(selected.includes(id) ? selected.filter(key => key !== id) : [...selected, id]);
      anchor.current = id;
    } else {
      setBatch(null);
      anchor.current = id;
    }
    onChooseSource(id);
  }
  async function move(ids: string[], folderId: string | null) {
    onEndDrag();
    if (!await edit({ type: "moveSources", ids, folderId })) return;
    setMoveIds(null);
    setBatch(null);
  }
  const [renaming, setRenaming] = useState<Asset | null>(null);
  const [mode, setMode] = useState<BinMode>(() => {
    try { return localStorage.getItem("cutroom.bin-view.v1") === "list" ? "list" : "grid"; } catch { return "grid"; }
  });
  function chooseMode(next: BinMode) {
    setMode(next);
    try { localStorage.setItem("cutroom.bin-view.v1", next); } catch { /* Keep the current view. */ }
  }
  return <section className="panel sources" id="source-pane" aria-labelledby="sources-title">
    <header className="panel-header"><h2 id="sources-title">소스 보관함</h2>
      <button className="button compact" disabled={busy} onClick={onImport}>＋ 가져오기</button>
      <div className="segmented bin-modes" role="group" aria-label="소스 보관함 보기">
        {(["grid", "list"] as const).map(value => <button key={value} type="button"
          aria-label={value === "grid" ? "격자 보기" : "목록 보기"} title={value === "grid" ? "격자 보기" : "목록 보기"}
          aria-pressed={mode === value} onClick={() => chooseMode(value)}>
          <span aria-hidden="true">{value === "grid" ? "⊞" : "☰"}</span>
          <span className="bin-mode-label">{value === "grid" ? "격자" : "목록"}</span>
        </button>)}
      </div>
    </header>
    {captureControls}
    {dragError && <div role="alert" className="folder-form">{dragError}
      <button className="text-button" onClick={() => setDragError("")}>닫기</button></div>}
    {moveIds && <DestinationPicker folders={folders} busy={busy} onMove={id => void move(moveIds,id)} close={() => setMoveIds(null)}/>}
    <BinFolders key={active ?? "root"} controls={folderControls} mode={mode} busy={busy} onMenu={onMenu} move={move}
      selectedSources={selected} selectSources={selectFiles} finishSelection={ids => {
        selectFiles(ids);
        anchor.current = ids[0] ?? null;
        if (ids.length === 1) onChooseSource(ids[0]);
      }} empty={visible.length === 0} selected={selectedFolder} select={id => {
        setSelectedFolder(id); setBatch([]); setMoveIds(null); setRenaming(null); anchor.current = null;
      }}>
      {visible.map(asset => (
        <button
          key={asset.id} data-source-id={asset.id}
          className={`source-item ${selection.has(asset.id) ? "selected" : ""}`}
          aria-label={assetLabels[asset.kind] + " · " + asset.name}
          aria-pressed={selection.has(asset.id)}
          draggable={!busy}
          title={supportsFileDrag() ? "컷룸 안에 배치하거나 탐색기 · 다른 앱으로 끌어 복사" : "트랙으로 끌어 놓기"}
          onClick={event => selectAsset(event, asset.id)}
          aria-keyshortcuts="Delete F2"
          onKeyDown={(event) => {
            if (event.key === "F2" && !busy) { event.preventDefault(); setRenaming(asset); }
            if (event.key === "Delete" && !event.repeat && !busy) { event.preventDefault(); onRemoveSource(asset.id); }
          }}
          onContextMenu={(event) => {
            event.preventDefault();
            if (!busy) onMenu({ x: event.clientX, y: event.clientY, items: [{ label: "폴더로 이동", run: () => setMoveIds(selection.has(asset.id) ? selected : [asset.id]) }, { label: "이름 바꾸기", run: () => setRenaming(asset) }, { label: "소스 카드 삭제", run: () => onRemoveSource(asset.id) }] });
          }}
          onDragStart={(event) => {
            const ids = selection.has(asset.id) ? selected : [asset.id];
            if (supportsFileDrag()) {
              event.preventDefault();
              setDragError("");
              void dragBinFiles(asset.id, ids, () => onStartDrag(asset.id), onEndDrag)
                .catch(error => setDragError(String(error)));
              return;
            }
            writeBinDrag(event.dataTransfer, asset.id, ids);
            onStartDrag(asset.id);
          }}
          onDragEnd={supportsFileDrag() ? undefined : onEndDrag}
        >
          {asset.missing && <span className="home-error">파일 누락</span>}
          <BinThumbnail asset={asset} mediaDir={mediaDir} mode={mode} />
          <span className="bin-name" title={asset.name}>{asset.name}</span>
        </button>
      ))}
    </BinFolders>
    {renaming && <SourceRename key={renaming.id} asset={renaming} busy={busy} save={onRenameSource} close={() => setRenaming(null)} />}
    {selected.length > 0 && <footer className="source-footer">
      <button className="button compact" disabled={busy} onClick={() => setMoveIds(selected)}>
        {selected.length > 1 ? selected.length + "개 이동" : "폴더로 이동"}</button>
      {selected.length === 1 && source?.id === selected[0] && <>
        {source.missing && <button className="button primary" disabled={busy} onClick={() => onRelinkSource(source.id)}>파일 다시 연결</button>}
        <button className="button wide" disabled={!hasScene || busy || source.missing} onClick={onLink}>{hasScene ? "선택한 구간에 넣기" : "대본 구간을 먼저 선택하세요"}</button>
        <button className="button compact" disabled={busy} onClick={() => onRemoveSource(source.id)}>카드 삭제</button>
      </>}
    </footer>}
  </section>;
}

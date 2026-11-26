import { useEffect, useState, type RefObject } from "react";
import { SceneTitleInput, type SceneRename } from "./SceneTitleInput";
import { Filmstrip } from "./Filmstrip";
import { TimelineRuler } from "./TimelineRuler";
import { useReorderDrag } from "./useReorderDrag";
import { sceneLabel, sourceDragType, sourceUrl, type Asset, type Scene } from "./project";
import type { MenuAction } from "./ClipMenu";
import type { timelineSections } from "./timeline";
import { visualClips, trackNumbers, endOf } from "./visualTracks";
import { sourceTime, videoRange } from "./videoTrim";

type Props = {
  sections: ReturnType<typeof timelineSections>;
  assets: Map<string, Asset>;
  mediaDir: string;
  selectedId?: string;
  scrollRef: RefObject<HTMLDivElement | null>;
  pixelsPerMs: number;
  viewport: { left: number; right: number };
  onSeek: (ms: number) => void;
  time: number;
  playing: boolean;
  busy: boolean;
  dragAsset?: Asset;
  select: (id: string, time: number) => void;
  onOpenDetail: (id: string, time: number) => void;
  add: (sceneId: string, asset: Asset) => void;
  onImport: () => void;
  rename: SceneRename;
  onRemove: (scene: Scene) => void;
  onMove: (id: string, targetId: string, after: boolean) => void;
  onMenu: (menu: MenuAction) => void;
};

export function TimelineOverview({ sections, assets, mediaDir, selectedId, scrollRef, pixelsPerMs, viewport, onSeek, time, playing, busy, dragAsset, select, onOpenDetail, add, onImport, onRemove, rename, onMenu, onMove }: Props) {
  const [picker, setPicker] = useState<string | null>(null);
  const reorder = useReorderDrag("application/x-cutroom-scene", "overview", sections.map(entry => entry.scene.id), busy, onMove);
  const activePicker = sections.find((entry) => entry.scene.id === picker && picker === selectedId);
  const duration = sections[sections.length - 1]?.end ?? 0;
  const sectionClips = sections.map(entry => visualClips(entry.scene));
  const lanes = trackNumbers(sectionClips.flat());
  const head = time * pixelsPerMs;
  useEffect(() => {
    const node = scrollRef.current;
    if (playing && node && (head < node.scrollLeft || head >= node.scrollLeft + node.clientWidth)) {
      node.scrollLeft = Math.max(0, head - node.clientWidth * 0.15);
    }
  }, [head, playing, scrollRef]);

  return <div className="overview-body">
    <div className="overview-scroll" ref={scrollRef}>
      {duration === 0 ? <p className="overview-empty">대본에서 구간을 추가하면 여기에 표시됩니다.</p> :
        <div className="overview-strip" style={{ width: duration * pixelsPerMs }}>
          <TimelineRuler pixelsPerMs={pixelsPerMs} viewport={viewport} duration={duration} time={time} onSeek={onSeek} label="전체 타임라인 재생바" />
          <div className="overview-sections">
          {sections.map(entry => {
            const id = entry.scene.id;
            const label = sceneLabel(entry.scene, entry.index);
            const editingName = rename.active?.id === id && rename.active.surface === "timeline";
            const width = entry.durationMs * pixelsPerMs;
            return <div className="overview-section" key={id} style={{ width, marginLeft: entry.start * pixelsPerMs }} data-selected={id === selectedId} {...reorder(id)} draggable={!editingName && !busy && sections.length > 1}
              role="group" aria-label={label} aria-keyshortcuts="Delete"
              onDoubleClick={(event) => {
                if ((event.target as Element).closest("input, .overview-add")) return;
                const at = id === selectedId && time >= entry.start && time < entry.end ? time : entry.start;
                onOpenDetail(id, at);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                if (busy) return;
                setPicker(null);
                select(id, entry.start);
                onMenu({ x: event.clientX, y: event.clientY, items: [{ label: "이름 바꾸기", run: () => rename.start(entry.scene, "timeline") }, { label: "구간 삭제", run: () => onRemove(entry.scene) }] });
              }}
              onKeyDown={(event) => {
                if (event.key !== "Delete" || event.repeat || busy) return;
                event.preventDefault();
                event.stopPropagation();
                select(id, id === selectedId ? time : entry.start);
                onRemove(entry.scene);
              }}
              onDragOver={(event) => { if (!busy && dragAsset) event.preventDefault(); }}
              onDrop={(event) => {
                if (busy || !dragAsset || event.dataTransfer.getData(sourceDragType) !== dragAsset.id) return;
                event.preventDefault(); select(id, entry.start); add(id, dragAsset); setPicker(null);
              }}>
              {editingName ? <SceneTitleInput rename={rename} fallback={`구간 ${entry.index + 1}`} busy={busy} /> :
              <button data-scene-title className="overview-label" aria-pressed={id === selectedId} title={entry.scene.text}
                aria-keyshortcuts="F2" onKeyDown={event => {
                  if (event.key !== "F2" || event.repeat || busy) return;
                  event.preventDefault();
                  setPicker(null);
                  select(id, id === selectedId ? time : entry.start);
                  rename.start(entry.scene, "timeline");
                }}
                onClick={() => { setPicker(null); select(id, entry.start); }}>
                <strong title={label}>{label}</strong><small>{sourceTime(entry.start)}–{sourceTime(entry.end)}</small>
              </button>}
              {lanes.map(track => <div className="overview-clips" key={track}>
                {track === 0 && !sectionClips[entry.index].some(c => c.track === 0) && <button className="overview-add" disabled={busy}
                  aria-label={`${label}에 영상 또는 이미지 추가`} onClick={()=>{select(id,entry.start);setPicker(picker === id ? null : id);}}>＋ 영상 추가</button>}
                {sectionClips[entry.index].filter(c => c.track === track).map(clip => {
                  const asset = assets.get(clip.assetId);
                  if (!asset) return null;
                  const bounds = videoRange(clip,clip.durationMs,asset.durationMs);
                  const begin = clip.startMs+bounds.offsetMs;
                  const length = asset.kind === "video" ? bounds.outMs-bounds.inMs : clip.durationMs;
                  const filmLeft = (entry.start + begin) * pixelsPerMs;
                  return <button className="overview-clip" key={clip.id} style={{left:begin*pixelsPerMs,width:length*pixelsPerMs}}
                    aria-label={`${label} · ${asset.name}`} title={asset.name} onClick={event => {
                      const offset = event.detail ? Math.max(0,(event.clientX-event.currentTarget.getBoundingClientRect().left)/pixelsPerMs) : 0;
                      select(id,entry.start+Math.min(endOf(clip)-1,begin+Math.floor(offset))); setPicker(null);
                    }}>
                    {asset.kind === "image" ? <img src={sourceUrl(mediaDir,asset)} alt="" draggable={false}/> :
                      <Filmstrip assetId={asset.id} inMs={bounds.inMs} outMs={bounds.outMs} pixelsPerMs={pixelsPerMs}
                        visibleFrom={viewport.left-filmLeft} visibleTo={viewport.right-filmLeft}/>}
                  </button>;
                })}
              </div>)}
            </div>;
          })}
          </div>
          <i className="overview-head" aria-hidden="true" style={{ left: Math.max(0, Math.min(duration * pixelsPerMs - 2, head - 1)) }} />
        </div>}
    </div>
    {activePicker && <div className="overview-picker" role="group" aria-label={`${sceneLabel(activePicker.scene, activePicker.index)} 소스 선택`}>
      {Array.from(assets.values()).filter((asset) => !asset.removedFromBin && asset.kind !== "audio").map((asset) =>
        <button key={asset.id} className="button compact" disabled={busy} onClick={() => { add(activePicker.scene.id, asset); setPicker(null); }}>{asset.name}</button>)}
      <button className="button compact" disabled={busy} onClick={onImport}>파일 가져오기</button>
      <button className="text-button" onClick={() => setPicker(null)}>닫기</button>
    </div>}
  </div>;
}

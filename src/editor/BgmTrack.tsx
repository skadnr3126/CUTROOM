import { useEffect, useRef, useState, type DragEvent } from "react";
import { TimeGrip } from "./TimeGrip";
import { BgmSourceRange } from "./BgmSourceRange";
import { anchorAt, anchorTime, moveBgm, type Sections } from "./bgm";
import { sourceDragType, sourceUrl, type Asset, type BgmAnchor, type BgmClip } from "./project";
import { sourceTime } from "./videoTrim";
import "./BgmTrack.css";

const bgmDrag = "application/x-cutroom-bgm-clip";

type Props = {
  sections: Sections; clips: BgmClip[]; assets: Map<string, Asset>; scale: number; busy: boolean;
  dragAsset?: Asset; time: number; mediaDir: string; onChange: (clips: BgmClip[]) => void;
};
export function BgmTrack({sections, clips, assets, scale, busy, dragAsset, time, mediaDir, onChange}: Props) {
  const lane = useRef<HTMLDivElement>(null);
  const [selected, select] = useState<string>();
  const [draft, setDraft] = useState<BgmClip>();
  const [pending, setPending] = useState<{ asset: Asset; start: BgmAnchor; id: string }>();
  const [notice, setNotice] = useState("");
  function durationUnavailable() {
    setPending(undefined);
    setNotice("원본 음악의 길이를 확인할 수 없습니다.");
  }
  useEffect(() => {
    if (!pending) return;
    const timeout = window.setTimeout(durationUnavailable, 5000);
    return () => window.clearTimeout(timeout);
  }, [pending]);
  const move = useRef<{id: string; gripMs: number} | null>(null);
  const last = sections[sections.length-1];
  const active = clips.find(c => c.id === selected);
  const activeStart = active ? anchorTime(active.start, sections) : 0;
  const activeEnd = active ? anchorTime(active.end, sections) : 0;
  const songs = Array.from(assets.values()).filter(a => a.kind === "audio" && !a.removedFromBin && !a.missing);
  function atX(x: number) {return (x - lane.current!.getBoundingClientRect().left) / scale;}
  function place(assetId: string, start: BgmAnchor, durationMs: number) {
    if (!Number.isFinite(durationMs) || durationMs < 1) {
      durationUnavailable();
      return;
    }
    if (busy || !sections.some(s => s.scene.id === start.sceneId) || !songs.some(s => s.id === assetId)) {
      setNotice("타임라인이 변경되었습니다. BGM을 다시 추가해 주세요.");
      return;
    }
    const from = anchorTime(start, sections);
    if (from >= last.end) {
      setNotice("타임라인 끝보다 앞에 BGM을 놓아 주세요.");
      return;
    }
    const end = anchorAt(sections, from + durationMs, "end", 0, false);
    const clip: BgmClip = {id:crypto.randomUUID(),assetId,start,end,volume:35,fadeInMs:0,fadeOutMs:0};
    onChange([...clips,clip]); select(clip.id);
  }
  function add(asset: Asset, at: number) {
    if (busy || pending || !last) return;
    setNotice("");
    if (at >= last.end) {
      setNotice("타임라인 끝보다 앞에 BGM을 놓아 주세요.");
      return;
    }
    const snapped = anchorAt(sections, at, "start", 8/scale);
    const start = anchorTime(snapped, sections) < last.end ? snapped : anchorAt(sections, Math.min(at, last.end-1), "start", 0);
    if (asset.durationMs && asset.durationMs > 0) place(asset.id, start, asset.durationMs);
    else setPending({ asset, start, id: crypto.randomUUID() });
  }
  function save(clip: BgmClip) { onChange(clips.map(c => c.id === clip.id ? clip : c)); }
  function remove(id: string) { onChange(clips.filter(clip => clip.id !== id)); }
  function dragClip(event: DragEvent, commit: boolean) {
    if (busy || !move.current || !event.dataTransfer.types.includes(bgmDrag)) return;
    const clip = clips.find(c => c.id === move.current?.id);
    if (!clip || (commit && event.dataTransfer.getData(bgmDrag) !== clip.id)) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    const next = moveBgm(clip, sections, atX(event.clientX) - move.current.gripMs, 8/scale);
    if (commit) {
      move.current = null;
      setDraft(undefined);
      if (next !== clip) save(next);
    } else {
      setDraft(next);
    }
  }
  function resized(clip: BgmClip, edge: "start" | "end", ms: number) {
    const anchor = anchorAt(sections, ms, edge, 8/scale);
    const next = {...clip,[edge]:anchor};
    return anchorTime(next.start, sections) < anchorTime(next.end, sections) ? next : undefined;
  }
  function handle(clip: BgmClip, edge: "start" | "end") {
    const at = anchorTime(clip[edge],sections);
    return <TimeGrip className={"bgm-edge "+edge} disabled={busy} scale={scale}
      label={edge === "start" ? "BGM 시작점 조절" : "BGM 끝점 조절"} onSelect={()=>select(clip.id)}
      homeDelta={edge==="start"?-at:undefined} endDelta={edge==="end"?last.end-at:undefined}
      onCancel={()=>setDraft(undefined)} onDelta={(delta,commit)=>{
        const next=resized(clip,edge,at+delta);
        setDraft(commit?undefined:next);
        if (commit && next) save(next);
      }}/>;
  }
  return <>
    <div ref={lane} className="bgm-lane" aria-label="전체 타임라인 BGM"
      onDragOver={e => {if (move.current) dragClip(e,false); else if (!busy && !pending && dragAsset?.kind === "audio") e.preventDefault();}}
      onDragLeave={e => {if (move.current && !e.currentTarget.contains(e.relatedTarget as Node | null)) setDraft(undefined);}}
      onDrop={e => {
        if (move.current) { dragClip(e,true); return; }
        const asset = assets.get(e.dataTransfer.getData(sourceDragType));
        if (asset?.kind === "audio" && !busy) {e.preventDefault();e.stopPropagation();add(asset,atX(e.clientX));}
      }}>
      {!clips.length && <span>BGM · 오디오를 여기에 놓으세요</span>}
      {clips.map(clip => {
        const shown = draft?.id === clip.id ? draft : clip;
        const start = anchorTime(shown.start,sections), end = anchorTime(shown.end,sections);
        return <div className="bgm-row" key={clip.id}><div className="bgm-clip" data-active={active?.id===clip.id}
          onFocus={()=>select(clip.id)} onKeyDown={event=>{
            if (event.key !== "Delete" || event.defaultPrevented || event.nativeEvent.isComposing ||
                event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
            event.preventDefault();
            event.stopPropagation();
            if (!busy && !event.repeat) remove(clip.id);
          }}
          style={{left:start*scale,width:Math.max(2,(end-start)*scale)}}>
          {handle(clip,"start")}<button type="button" className="bgm-body" disabled={busy} draggable={!busy}
            aria-pressed={active?.id===clip.id} aria-keyshortcuts="Delete" onClick={() => select(clip.id)}
            title={sourceTime(start)+" – "+sourceTime(end)+" · 드래그하여 이동"}
            onDragStart={e=>{
              select(clip.id);
              move.current={id:clip.id,gripMs:atX(e.clientX)-anchorTime(clip.start,sections)};
              e.dataTransfer.setData(bgmDrag,clip.id);
              e.dataTransfer.effectAllowed="move";
            }}
            onDragEnd={()=>{move.current=null;setDraft(undefined);}}>
            {assets.get(clip.assetId)?.name ?? "BGM"}</button>{handle(clip,"end")}
        </div></div>;
      })}
    </div>
    <div className="bgm-tools">
      <select aria-label="BGM 추가" value="" disabled={busy || !!pending || !songs.length} onChange={e => {const song=assets.get(e.target.value); if(song) add(song,time);}}>
        <option value="">＋ BGM 추가</option>{songs.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}
      </select>
      {pending && <audio key={pending.id} preload="metadata" src={sourceUrl(mediaDir, pending.asset)}
        onLoadedMetadata={e => {
          setPending(undefined);
          place(pending.asset.id, pending.start, Math.round(e.currentTarget.duration * 1000));
        }} onError={durationUnavailable} />}
      {(pending || notice) && <span role="status">{pending ? "원본 음악 길이 확인 중…" : notice}</span>}
      {active && <>
        {(["start","end"] as const).map(edge => <label key={edge}>{edge === "start" ? "시작" : "끝"} <select disabled={busy} value={active[edge].sceneId}
          onChange={e => save({...active,[edge]:{sceneId:e.target.value,offsetMs:edge==="end"?null:0}})}>
          {sections.map(s => <option key={s.scene.id} value={s.scene.id} disabled={edge==="start"?s.start>=activeEnd:s.end<=activeStart}>
            {s.scene.name || "구간 "+(s.index+1)}</option>)}</select></label>)}
        {(["volume","fadeInMs","fadeOutMs"] as const).map(field => {
          const volume = field==="volume", factor=volume?1:1000;
          return <label key={field}>{volume?"음량 %":field==="fadeInMs"?"페이드 인(초)":"페이드 아웃(초)"} <input
            key={active.id+":"+active[field]} type="number" min="0" max={volume?100:86400} step={volume?1:0.1}
            disabled={busy} defaultValue={active[field]/factor} onKeyDown={e=>{if(e.key==="Enter") e.currentTarget.blur();}}
            onBlur={e=>{const input=e.currentTarget; if(input.validity.valid && Number.isFinite(input.valueAsNumber)){
              const value=Math.round(input.valueAsNumber*factor); if(value!==active[field]) save({...active,[field]:value});
            } else input.value=String(active[field]/factor);}}/></label>;
        })}
        <button className="button compact" disabled={busy} onClick={()=>remove(active.id)}>BGM 삭제 <kbd>Delete</kbd></button>
        <BgmSourceRange key={active.id} clip={active} duration={assets.get(active.assetId)?.durationMs ?? 0} busy={busy} save={save}/>
      </>}
    </div>
  </>;
}

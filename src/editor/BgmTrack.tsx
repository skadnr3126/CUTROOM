import { useRef, useState } from "react";
import { TimeGrip } from "./TimeGrip";
import { BgmSourceRange } from "./BgmSourceRange";
import { anchorAt, anchorTime, type Sections } from "./bgm";
import { sourceDragType, type Asset, type BgmClip } from "./project";
import { sourceTime } from "./videoTrim";
import "./BgmTrack.css";

type Props = {
  sections: Sections; clips: BgmClip[]; assets: Map<string, Asset>; scale: number; busy: boolean;
  dragAsset?: Asset; time: number; onChange: (clips: BgmClip[]) => void;
};
export function BgmTrack({sections, clips, assets, scale, busy, dragAsset, time, onChange}: Props) {
  const lane = useRef<HTMLDivElement>(null);
  const [selected, select] = useState<string>();
  const [draft, setDraft] = useState<BgmClip>();
  const [rangeFor, setRangeFor] = useState<string>();
  const rangeOrigin = useRef<number | null>(null);
  const last = sections[sections.length-1];
  const active = clips.find(c => c.id === selected);
  const activeStart = active ? anchorTime(active.start, sections) : 0;
  const activeEnd = active ? anchorTime(active.end, sections) : 0;
  const songs = Array.from(assets.values()).filter(a => a.kind === "audio" && !a.removedFromBin && !a.missing);
  function atX(x: number) {return (x - lane.current!.getBoundingClientRect().left) / scale;}
  function add(asset: Asset, at: number) {
    if (busy || !last) return;
    let start = anchorAt(sections, at, "start", 8/scale);
    if (anchorTime(start, sections) >= last.end) start = {sceneId:last.scene.id,offsetMs:0};
    const clip: BgmClip = {id:crypto.randomUUID(),assetId:asset.id,start,end:{sceneId:last.scene.id,offsetMs:null},volume:35,fadeInMs:0,fadeOutMs:0};
    onChange([...clips,clip]); select(clip.id);
  }
  function save(clip: BgmClip) { onChange(clips.map(c => c.id === clip.id ? clip : c)); }
  function drawnPlacement(clip: BgmClip, x: number) {
    const a = rangeOrigin.current, b = atX(x);
    if (a === null || a === b) return;
    const start = anchorAt(sections,Math.min(a,b),"start",8/scale);
    const end = anchorAt(sections,Math.max(a,b),"end",8/scale);
    if (anchorTime(start,sections) < anchorTime(end,sections)) return {...clip,start,end};
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
      onDragOver={e => {if (!busy && dragAsset?.kind === "audio") e.preventDefault();}}
      onDrop={e => {
        const asset = assets.get(e.dataTransfer.getData(sourceDragType));
        if (asset?.kind === "audio" && !busy) {e.preventDefault();e.stopPropagation();add(asset,atX(e.clientX));}
      }}>
      {!clips.length && <span>BGM · 오디오를 여기에 놓으세요</span>}
      {clips.map(clip => {
        const shown = draft?.id === clip.id ? draft : clip;
        const start = anchorTime(shown.start,sections), end = anchorTime(shown.end,sections);
        return <div className="bgm-row" key={clip.id}><div className="bgm-clip" data-active={active?.id===clip.id}
          style={{left:start*scale,width:Math.max(2,(end-start)*scale)}}>
          {handle(clip,"start")}<button disabled={busy} onClick={() => select(clip.id)} title={sourceTime(start)+" – "+sourceTime(end)}>
            {assets.get(clip.assetId)?.name ?? "BGM"}</button>{handle(clip,"end")}
        </div></div>;
      })}
      {active && rangeFor===active.id && !busy && <button type="button" className="bgm-draw-range"
        aria-label="BGM 배치 범위를 타임라인에서 드래그" title="사용할 타임라인 범위를 드래그하세요"
        onKeyDown={e=>{if(e.key==="Escape"){setRangeFor(undefined);setDraft(undefined);}}}
        onPointerDown={e=>{if(e.button!==0) return;rangeOrigin.current=atX(e.clientX);e.currentTarget.setPointerCapture(e.pointerId);}}
        onPointerMove={e=>{if(e.currentTarget.hasPointerCapture(e.pointerId)) setDraft(drawnPlacement(active,e.clientX));}}
        onPointerUp={e=>{
          if(!e.currentTarget.hasPointerCapture(e.pointerId)) return;
          const next=drawnPlacement(active,e.clientX);
          rangeOrigin.current=null;setDraft(undefined);
          if(next){setRangeFor(undefined);save(next);}
        }}
        onLostPointerCapture={()=>{rangeOrigin.current=null;setDraft(undefined);}}/>}
    </div>
    <div className="bgm-tools">
      <select aria-label="BGM 추가" value="" disabled={busy || !songs.length} onChange={e => {const song=assets.get(e.target.value); if(song) add(song,time);}}>
        <option value="">＋ BGM 추가</option>{songs.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}
      </select>
      {active && <>
        <button type="button" className="button compact" disabled={busy} aria-pressed={rangeFor===active.id}
          onClick={()=>{setRangeFor(rangeFor===active.id?undefined:active.id);setDraft(undefined);}}>
          {rangeFor===active.id?"범위 지정 취소":"배치 범위 드래그"}</button>
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
        <button className="button compact" disabled={busy} onClick={()=>onChange(clips.filter(c=>c.id!==active.id))}>BGM 삭제</button>
        <BgmSourceRange key={active.id} clip={active} duration={assets.get(active.assetId)?.durationMs ?? 0} busy={busy} save={save}/>
      </>}
    </div>
  </>;
}

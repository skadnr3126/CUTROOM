import { useLayoutEffect, useRef, useState } from "react";
import type { BgmClip, BgmSourceRange } from "./project";
import { sourceTime } from "./videoTrim";
import { TimeGrip } from "./TimeGrip";

type Props = {
  clip: BgmClip;
  duration: number;
  busy: boolean;
  save: (clip: BgmClip) => void;
};

export function BgmSourceRange({clip, duration, busy, save}: Props) {
  const bar = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [draft, setDraft] = useState<BgmSourceRange>();
  const origin = useRef<number | null>(null);
  const saved = clip.sourceRange ?? {inMs:0, outMs:duration};
  const range = draft ?? saved;
  useLayoutEffect(() => {
    const node = bar.current;
    if (!node) return;
    const observer = new ResizeObserver(() => setWidth(node.clientWidth));
    observer.observe(node);
    return () => observer.disconnect();
  }, [duration]);
  if (duration <= 0) return <span>곡 길이를 확인해야 원본 범위를 조절할 수 있습니다.</span>;
  const point = (x: number) => Math.min(duration, Math.max(0, Math.round((x-bar.current!.getBoundingClientRect().left)*duration/width)));
  function apply(next: BgmSourceRange, commit: boolean) {
    if (!commit) {setDraft(next);return;}
    setDraft(undefined);
    if (next.inMs !== saved.inMs || next.outMs !== saved.outMs) {
      save({...clip,sourceRange:next.inMs===0 && next.outMs===duration ? null : next});
    }
  }
  function drawn(x: number) {
    const a = origin.current!, b = point(x);
    return {inMs:Math.min(a,b),outMs:Math.max(a,b)};
  }
  return <div className="bgm-source-trim">
    <span>곡 사용 범위 · {sourceTime(range.inMs)} – {sourceTime(range.outMs)}</span>
    <div className="bgm-source-bar" ref={bar} role="group" aria-label="곡 사용 범위 드래그"
      onPointerDown={e => {
        if (busy || !width || e.button !== 0 || e.target instanceof Element && e.target.closest("button")) return;
        origin.current=point(e.clientX);e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={e => {if(origin.current!==null && e.currentTarget.hasPointerCapture(e.pointerId)) setDraft(drawn(e.clientX));}}
      onPointerUp={e => {
        if(origin.current===null || !e.currentTarget.hasPointerCapture(e.pointerId)) return;
        const next=drawn(e.clientX); origin.current=null;setDraft(undefined);
        if(next.outMs>next.inMs) apply(next,true);
      }}
      onLostPointerCapture={e=>{if(e.target===e.currentTarget){origin.current=null;setDraft(undefined);}}}>
      <div className="bgm-source-selection" style={{left:100*range.inMs/duration+"%",width:100*(range.outMs-range.inMs)/duration+"%"}}/>
      {(["inMs","outMs"] as const).map(edge => {
        const first = edge==="inMs";
        return <TimeGrip key={edge} className={"bgm-source-edge "+(first?"start":"end")}
          style={{left:100*range[edge]/duration+"%"}} label={first?"원곡 시작점":"원곡 끝점"}
          scale={width/duration} disabled={busy || !width} onSelect={()=>{}} onCancel={()=>setDraft(undefined)}
          homeDelta={first?-saved.inMs:undefined} endDelta={first?undefined:duration-saved.outMs}
          onDelta={(delta,commit)=>apply({...saved,[edge]:first?Math.max(0,Math.min(saved.outMs-1,saved.inMs+delta)):
            Math.min(duration,Math.max(saved.inMs+1,saved.outMs+delta))},commit)}/>;
      })}
    </div>
    <button className="button compact" disabled={busy || !clip.sourceRange} onClick={()=>save({...clip,sourceRange:null})}>곡 전체 사용</button>
  </div>;
}

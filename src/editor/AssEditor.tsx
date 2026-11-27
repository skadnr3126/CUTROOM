import { useEffect, useMemo, useRef, useState } from "react";
import { CaptionTextField } from "./CaptionTextField";
import { invoke } from "./projectSession";
import type { Scene } from "./project";
import { sourceTime } from "./videoTrim";
import { useCaptionAutosave } from "./useCaptionAutosave";
import { readAssRows, type AssRow as Row, type AssTrack as Track } from "./assRows";

export function useAssRows(track?: Track) {
  const source=track?.source;
  const [state,set] = useState<{source:string; rows:Row[]; error:string}>();
  useEffect(() => {
    if (!track || !source) return;
    let active=true;
    readAssRows(track).then(rows=>{
      if(active) set({source,rows,error:""});
    }).catch(error=>{if(active) set({source,rows:[],error:String(error)});});
    return ()=>{active=false;};
  }, [track,source]);
  return source && state?.source===source ? state : undefined;
}

export function AssEditor({track,scene,revision,at,initialLine,close,save,remove,preview,seek}:{
  track:Track;scene:Scene;revision:number;at:number;initialLine?:number;close:()=>void;
  save:(source:string,expected:number)=>Promise<number|null>;remove:(expected:number)=>Promise<boolean>;preview:(source:string)=>void;seek:(ms:number)=>void;
}) {
  const dialog=useRef<HTMLDialogElement>(null);
  const textInput=useRef<HTMLTextAreaElement>(null);
  const parsed=useAssRows(track);
  const [edits,setEdits]=useState<Record<number,Row>>({});
  const [mergedLines,setMergedLines]=useState<Set<number>>(()=>new Set());
  const changes=useMemo(()=>Object.values(edits),[edits]);
  const [selected,setSelected]=useState<number|undefined>(initialLine);
  const [notice,setNotice]=useState("");
  const [working,setWorking]=useState(false);
  const [compiled,setCompiled]=useState<{changes:typeof changes;source:string}>();
  // Keep the opened rows together while their times are being edited.
  const groups=useMemo(()=>{
    const byStart=new Map<number,Row[]>();
    for(const row of parsed?.rows ?? []){
      if(mergedLines.has(row.line))continue;
      const group=byStart.get(row.startMs);
      if(group)group.push(row);
      else byStart.set(row.startMs,[row]);
    }
    return byStart;
  },[parsed,mergedLines]);
  const originals=parsed?.rows;
  const row=originals?.find(r=>r.line===selected)??originals?.find(r=>r.startMs<=at && at<r.endMs)??originals?.[0];
  const group=row && groups.get(row.startMs);
  const valid=changes.every(r=>
    !r.text.trim() || Number.isFinite(r.startMs) && Number.isFinite(r.endMs) && r.startMs>=0 && r.startMs<r.endMs && r.endMs<=scene.durationMs);
  const autosave=useCaptionAutosave(changes,revision,scene.revision,valid,async(snapshot,expected)=>{
    const source=compiled?.changes===snapshot ? compiled.source :
      await invoke<string>("ass_update",{source:track.source,edits:snapshot,durationMs:scene.durationMs});
    return save(source,expected);
  });
  const stale=autosave.stale;
  const ready=compiled?.changes===changes && !stale && !working;
  useEffect(()=>{dialog.current?.showModal();},[]);
  useEffect(()=>{if (row) textInput.current?.focus();},[row?.line]);
  useEffect(()=>{
    let active=true;
    setNotice("");
    if(!changes.length){setCompiled({changes,source:track.source});preview(track.source);return;}
    const timer=window.setTimeout(()=>{
      invoke<string>("ass_update",{source:track.source,edits:changes,durationMs:scene.durationMs})
        .then(source=>{if(active){setCompiled({changes,source});preview(source);}})
        .catch(error=>{if(active)setNotice(String(error));});
    },120);
    return ()=>{active=false;clearTimeout(timer);};
  },[changes,track.source,scene.durationMs,preview]);
  function change(original:Row,patch:Partial<Row>){
    setEdits(all => {
      const next = { ...all };
      const targets = initialLine !== undefined && ("startMs" in patch || "endMs" in patch)
        ? group?.filter(sibling=>sibling.endMs===original.endMs) ?? [original] : [original];
      for(const target of targets) next[target.line]={...(all[target.line]??target),...patch};
      return next;
    });
    if(patch.startMs!==undefined && Number.isFinite(patch.startMs))seek(patch.startMs);
  }
  function mergeWithPrevious(upper:Row,lower:Row){
    const center=upper.center && lower.center
      ? {x:(upper.center.x+lower.center.x)/2,y:(upper.center.y+lower.center.y)/2}
      : undefined;
    setEdits(all=>({
      ...all,
      [upper.line]:{...upper,center,text:upper.text+"\n"+lower.text},
      [lower.line]:{...lower,text:""},
    }));
    setMergedLines(all=>new Set(all).add(lower.line));
    setSelected(upper.line);
    requestAnimationFrame(()=>textInput.current?.focus());
  }
  async function commit(file:boolean){
    if(!ready || !compiled)return;
    setWorking(true);
    try {
      if(file){
        const saved=await invoke<boolean>("ass_save_file",{track:{...track,source:compiled.source}});
        if(saved)setNotice("ASS 파일을 저장했습니다.");
      } else if(await autosave.flush())close();
      else setNotice("프로젝트에 저장하지 못했습니다.");
    }catch(e){setNotice(String(e));}finally{setWorking(false);}
  }
  async function centerOnPanel(line:number){
    setWorking(true);
    setNotice("배경과 자막 위치를 확인하고 있습니다…");
    try {
      const positions=await invoke<{line:number;center:NonNullable<Row["center"]>}[]>("ass_center_on_panel",{
        source:track.source,edits:changes,durationMs:scene.durationMs,line,
      });
      setEdits(all=>{
        const next={...all};
        for(const {line,center} of positions){
          const original=originals?.find(r=>r.line===line);
          if(original)next[line]={...(all[line]??original),center};
        }
        return next;
      });
    } catch(error){setNotice(String(error));}
    finally{setWorking(false);}
  }
  const visibleStart=(rows:Row[])=>Math.min(...rows.filter(r=>(edits[r.line]??r).text.trim()).map(r=>(edits[r.line]??r).startMs));
  const here=group && row ? visibleStart(group) : Infinity;
  const next=Array.from(groups.values()).filter(g=>g!==group).map(g=>({group:g,start:visibleStart(g)}))
    .filter(g=>Number.isFinite(g.start) && g.start>(Number.isFinite(here)?here:row?.startMs??at)).sort((a,b)=>a.start-b.start)[0];
  async function navigate(target?:Row[],start?:number){
    if(working || !await autosave.flush())return;
    if(!target){close();return;}
    setSelected(target[0].line);
    seek(start??(edits[target[0].line]??target[0]).startMs);
  }
  return <dialog className="caption-editor" ref={dialog} onCancel={e=>{e.preventDefault();void navigate();}}
    onCompositionStart={()=>autosave.setComposing(true)} onCompositionEnd={()=>autosave.setComposing(false)}>
    <h2>ASS 자막 편집 <small>{track.name}</small></h2>
    {row && group && <>
      <select aria-label="편집할 ASS 자막" disabled={working} value={row.startMs} onChange={e=>{
        void navigate(groups.get(Number(e.target.value)));
      }}>{Array.from(groups,([start,rows])=>{
        const first=edits[rows[0].line]??rows[0];
        return <option key={start} value={start}>{sourceTime(first.startMs)} · {first.text}{rows.length>1 && ` (자막 ${rows.length}개)`}</option>;
      })}</select>
      <div className="ass-edit-rows">{group.map((original,index)=>{
        const current=edits[original.line]??original;
        const previous=group[index-1];
        const upper=previous && (edits[previous.line]??previous);
        const label=group.length>1 ? `${index+1}번째 자막 문구` : "ASS 자막 문구";
        const deleting=!!edits[original.line] && !current.text.trim();
        return <div key={original.line} className="ass-edit-row">
          <div className="ass-row-actions">
            <strong>{label}</strong>
            <button type="button" className="text-button" disabled={stale||working||!valid||!current.text.trim()}
              onClick={()=>void centerOnPanel(original.line)}>배경 중앙에 맞추기</button>
            {upper?.text.trim() && current.text.trim() && upper.startMs===current.startMs && upper.endMs===current.endMs &&
              <button type="button" className="text-button" disabled={stale||working}
                title="위 자막의 스타일을 사용해 두 문구를 한 자막 안에 줄바꿈으로 합칩니다."
                onClick={()=>mergeWithPrevious(upper,current)}>위 자막과 합치기</button>}
            <button type="button" className="text-button" disabled={stale||working}
              onClick={()=>change(original,{text:deleting?original.text:""})}>{deleting?"원래 문구 복원":"이 자막 삭제"}</button>
          </div>
          <div className="cue-times">{(["startMs","endMs"] as const).map(key=><label key={key}>{key==="startMs"?"시작(초)":"끝(초)"}
            <input type="number" disabled={working||stale} min={0} max={scene.durationMs/1000} step="0.01" value={Number.isFinite(current[key])?current[key]/1000:""}
              onChange={e=>change(original,{[key]:Math.round(e.target.valueAsNumber*1000)})}/></label>)}</div>
          <CaptionTextField inputRef={original.line===row.line?textInput:undefined} label={label} value={current.text}
            rows={group.length>1?2:3} disabled={stale||working} placeholder={deleting?"삭제한 자막 · 문구를 입력해 복원":undefined}
            onChange={text=>change(original,{text})}/>
          {current.text.includes("{\\") && <small>문장 중간의 중괄호에는 글자별 효과가 들어 있습니다.</small>}
        </div>;
      })}</div>
    </>}
    {parsed && !row && !parsed.error && <p>편집할 자막 문구가 없습니다.</p>}
    <p role="status">{parsed?.error||notice||autosave.status}</p>
    <footer><button className="text-button" disabled={stale||working||autosave.writing} title="구간에서 분리합니다. 실행 취소할 수 있습니다."
        onClick={async()=>{setWorking(true);try{if(await autosave.flush() && await remove(autosave.revision.current))close();}finally{setWorking(false);}}}>전체 ASS 트랙 분리</button>
      <button className="button compact" disabled={!ready} onClick={()=>void commit(true)}>ASS 파일로 저장</button>
      <button className="button compact" disabled={working||!valid||!next||stale} onClick={()=>next && void navigate(next.group,next.start)}>다음 자막</button>
      {autosave.dirty && (!valid||stale||autosave.failure) && <button className="text-button" disabled={autosave.writing} onClick={close}>미저장 변경 버리고 닫기</button>}
      <button className="button compact" disabled={working} onClick={()=>void navigate()}>닫기</button>
      <button className="button compact primary" disabled={!ready} onClick={()=>void commit(false)}>프로젝트에 저장</button></footer>
  </dialog>;
}


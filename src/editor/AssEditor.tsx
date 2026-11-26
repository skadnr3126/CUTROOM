import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "./projectSession";
import type { NewAssRow, Scene } from "./project";
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
  const [added,setAdded]=useState<(NewAssRow & {id:string})[]>([]);
  const changes=useMemo(()=>({edits:Object.values(edits),additions:added.map(({templateLine,text})=>({templateLine,text}))}),[edits,added]);
  const [selected,setSelected]=useState<number|undefined>(initialLine);
  const [notice,setNotice]=useState("");
  const [working,setWorking]=useState(false);
  const [compiled,setCompiled]=useState<{changes:typeof changes;source:string}>();
  // Keep the opened rows together while their times are being edited.
  const groups=useMemo(()=>{
    const byStart=new Map<number,Row[]>();
    for(const row of parsed?.rows ?? []){
      const group=byStart.get(row.startMs);
      if(group)group.push(row);
      else byStart.set(row.startMs,[row]);
    }
    return byStart;
  },[parsed]);
  const originals=parsed?.rows;
  const row=originals?.find(r=>r.line===selected)??originals?.find(r=>r.startMs<=at && at<r.endMs)??originals?.[0];
  const group=row && groups.get(row.startMs);
  const valid=changes.additions.every(r=>r.text.trim()) && changes.edits.every(r=>
    !r.text.trim() || Number.isFinite(r.startMs) && Number.isFinite(r.endMs) && r.startMs>=0 && r.startMs<r.endMs && r.endMs<=scene.durationMs);
  const autosave=useCaptionAutosave(changes,revision,scene.revision,valid,async(snapshot,expected)=>{
    const source=compiled?.changes===snapshot ? compiled.source :
      await invoke<string>("ass_update",{source:track.source,...snapshot,durationMs:scene.durationMs});
    return save(source,expected);
  });
  const stale=autosave.stale;
  const ready=compiled?.changes===changes && !stale && !working;
  useEffect(()=>{dialog.current?.showModal();},[]);
  useEffect(()=>{if (row) textInput.current?.focus();},[row?.line]);
  useEffect(()=>{
    let active=true;
    setNotice("");
    if(!changes.edits.length && !changes.additions.length){setCompiled({changes,source:track.source});preview(track.source);return;}
    if(changes.additions.some(r=>!r.text.trim())){setNotice("추가한 줄에 문구를 입력해 주세요.");return;}
    const timer=window.setTimeout(()=>{
      invoke<string>("ass_update",{source:track.source,...changes,durationMs:scene.durationMs})
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
  const visibleStart=(rows:Row[])=>Math.min(...rows.filter(r=>(edits[r.line]??r).text.trim()||added.some(a=>a.templateLine===r.line)).map(r=>(edits[r.line]??r).startMs));
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
      <select aria-label="편집할 ASS 자막" value={row.startMs} onChange={e=>{
        void navigate(groups.get(Number(e.target.value)));
      }}>{Array.from(groups,([start,rows])=>{
        const first=edits[rows[0].line]??rows[0];
        return <option key={start} value={start}>{sourceTime(first.startMs)} · {first.text}{rows.length>1 && ` (${rows.length}줄)`}</option>;
      })}</select>
      <div className="ass-edit-rows">{group.map((original,index)=>{
        const current=edits[original.line]??original;
        const label=group.length>1 ? `${index+1}번째 줄` : "ASS 자막 문구";
        const deleting=!!edits[original.line] && !current.text.trim();
        return <div key={original.line} className="ass-edit-row">
          <div className="ass-row-actions">
            <strong>{label}</strong>
            <button type="button" className="text-button" disabled={stale||working}
              onClick={()=>setAdded(all=>[...all,{id:crypto.randomUUID(),templateLine:original.line,text:""}])}>＋ 줄 추가</button>
            <button type="button" className="text-button" disabled={stale||working}
              onClick={()=>change(original,{text:deleting?original.text:""})}>{deleting?"원래 문구 복원":"이 줄 삭제"}</button>
          </div>
          <div className="cue-times">{(["startMs","endMs"] as const).map(key=><label key={key}>{key==="startMs"?"시작(초)":"끝(초)"}
            <input type="number" min={0} max={scene.durationMs/1000} step="0.01" value={Number.isFinite(current[key])?current[key]/1000:""}
              onChange={e=>change(original,{[key]:Math.round(e.target.valueAsNumber*1000)})}/></label>)}</div>
          <textarea ref={original.line===row.line?textInput:undefined} aria-label={label} value={current.text} rows={group.length>1?2:3}
            placeholder={deleting?"삭제한 줄 · 문구를 입력해 복원":undefined} onChange={e=>change(original,{text:e.target.value})}/>
          {current.text.includes("{\\") && <small>문장 중간의 중괄호에는 글자별 효과가 들어 있습니다.</small>}
          {added.filter(r=>r.templateLine===original.line).map((r,i)=><div className="ass-added-row" key={r.id}>
            <div className="ass-row-actions"><strong>새 줄 {i+1}</strong>
              <button type="button" className="text-button" disabled={working}
                onClick={()=>setAdded(all=>all.filter(item=>item.id!==r.id))}>추가 취소</button></div>
            <textarea autoFocus rows={2} aria-label={`${label}에 추가한 새 줄 ${i+1}`} value={r.text} placeholder="새 줄에 표시할 문구"
              onChange={e=>setAdded(all=>all.map(item=>item.id===r.id?{...item,text:e.target.value}:item))}/>
          </div>)}
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


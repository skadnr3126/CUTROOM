import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "./projectSession";
import type { CaptionCue, Scene } from "./project";

export type AssRow = CaptionCue & { line: number };
type Row = AssRow;
type Track = NonNullable<Scene["captionAss"]>;
export function useAssRows(source?: string) {
  const [state,set] = useState<{source:string; rows:Row[]; error:string}>();
  useEffect(() => {
    if (!source) return;
    let active=true;
    invoke<Row[]>("ass_rows",{source}).then(rows=>{
      if(active) set({source,rows,error:""});
    }).catch(error=>{if(active) set({source,rows:[],error:String(error)});});
    return ()=>{active=false;};
  }, [source]);
  return source && state?.source===source ? state : undefined;
}

export function AssEditor({track,scene,revision,at,initialLine,close,save,remove,preview,seek}:{
  track:Track;scene:Scene;revision:number;at:number;initialLine?:number;close:()=>void;
  save:(edits:Row[])=>Promise<boolean>;remove:()=>Promise<boolean>;preview:(source:string)=>void;seek:(ms:number)=>void;
}) {
  const dialog=useRef<HTMLDialogElement>(null);
  const textInput=useRef<HTMLTextAreaElement>(null);
  const parsed=useAssRows(track.source);
  const [edits,setEdits]=useState<Record<number,Row>>({});
  const [selected,setSelected]=useState<number|undefined>(initialLine);
  const [notice,setNotice]=useState("");
  const [working,setWorking]=useState(false);
  const [compiled,setCompiled]=useState<{edits:typeof edits;source:string}>();
  const rows=useMemo(()=>parsed?.rows.map(row=>edits[row.line]??row)??[],[parsed,edits]);
  const row=rows.find(r=>r.line===selected)??rows.find(r=>r.startMs<=at && at<r.endMs)??rows[0];
  const stale=revision!==scene.revision;
  const ready=compiled?.edits===edits && !stale && !working;
  useEffect(()=>{dialog.current?.showModal();},[]);
  useEffect(()=>{if (row) textInput.current?.focus();},[row?.line]);
  useEffect(()=>{
    let active=true;
    setNotice("");
    if(!Object.keys(edits).length){setCompiled({edits,source:track.source});preview(track.source);return;}
    const timer=window.setTimeout(()=>{
      invoke<string>("ass_update",{source:track.source,edits:Object.values(edits),durationMs:scene.durationMs})
        .then(source=>{if(active){setCompiled({edits,source});preview(source);}})
        .catch(error=>{if(active)setNotice(String(error));});
    },120);
    return ()=>{active=false;clearTimeout(timer);};
  },[edits,track.source,scene.durationMs,preview]);
  function change(patch:Partial<Row>){
    if(!row)return;
    setSelected(row.line);
    const originals = parsed?.rows ?? [];
    const original = originals.find(r => r.line === row.line);
    setEdits(all => {
      const next = { ...all, [row.line]: { ...row, ...patch } };
      if (initialLine !== undefined && original && ("startMs" in patch || "endMs" in patch)) {
        for (const sibling of originals) {
          if (sibling.line !== row.line && sibling.startMs === original.startMs && sibling.endMs === original.endMs)
            next[sibling.line] = { ...(all[sibling.line] ?? sibling), ...patch };
        }
      }
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
      } else if(!Object.keys(edits).length || await save(Object.values(edits)))close();
      else setNotice("프로젝트에 저장하지 못했습니다.");
    }catch(e){setNotice(String(e));}finally{setWorking(false);}
  }
  return <dialog className="caption-editor" ref={dialog} onCancel={e=>{e.preventDefault();if(!working)close();}}>
    <h2>ASS 자막 편집 <small>{track.name}</small></h2>
    {row && <>
      <select aria-label="편집할 ASS 자막" value={row.line} onChange={e=>{
        const r=rows.find(item=>item.line===Number(e.target.value))!;
        setSelected(r.line);seek(r.startMs);
      }}>{rows.map((r,i)=><option key={r.line} value={r.line}>{i+1} · {r.text}</option>)}</select>
      <div className="cue-times">{(["startMs","endMs"] as const).map(key=><label key={key}>{key==="startMs"?"시작(초)":"끝(초)"}
        <input type="number" min={0} max={scene.durationMs/1000} step="0.01" value={Number.isFinite(row[key])?row[key]/1000:""}
          onChange={e=>change({[key]:Math.round(e.target.valueAsNumber*1000)})}/></label>)}</div>
      <textarea ref={textInput} aria-label="ASS 자막 문구" value={row.text} rows={3} onChange={e=>change({text:e.target.value})}/>
      {row.text.includes("{\\") && <small>문장 중간의 중괄호에는 글자별 효과가 들어 있습니다.</small>}
    </>}
    {parsed && !row && !parsed.error && <p>편집할 자막 문구가 없습니다.</p>}
    <p role="status">{stale?"구간이 변경되었습니다. 닫고 다시 열어 주세요.":parsed?.error||notice}</p>
    <footer><button className="text-button" disabled={stale||working} title="구간에서 분리합니다. 실행 취소할 수 있습니다."
        onClick={async()=>{setWorking(true);try{if(await remove())close();}finally{setWorking(false);}}}>자막 분리</button>
      <button className="button compact" disabled={!ready} onClick={()=>void commit(true)}>ASS 파일로 저장</button>
      <button className="button compact" disabled={working} onClick={close}>닫기</button>
      <button className="button compact primary" disabled={!ready} onClick={()=>void commit(false)}>프로젝트에 저장</button></footer>
  </dialog>;
}


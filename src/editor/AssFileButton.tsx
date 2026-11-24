import { useRef, useState } from "react";
import type { Scene, SceneTool } from "./project";

export function AssFileButton({scene,busy,save}:{
  scene:Scene; busy:boolean; save:(scene:Scene,tool:SceneTool)=>Promise<boolean>;
}){
  const file=useRef<HTMLInputElement>(null);
  const target=useRef(scene);
  const [error,setError]=useState("");
  async function read(input: File){
    const selected=target.current;
    try {
      if(input.size>2_000_000)throw Error("ASS 파일은 2MB까지 불러올 수 있습니다.");
      const source=new TextDecoder("utf-8",{fatal:true}).decode(await input.arrayBuffer());
      if(!await save(selected,{type:"setAssTrack",name:input.name,source}))setError("ASS 자막을 불러오지 못했습니다. 프로젝트 알림을 확인해 주세요.");
    }catch(e){setError(String(e));}
  }
  return <><input hidden ref={file} type="file" accept=".ass" onChange={e=>{
    const input=e.currentTarget.files?.[0]; e.currentTarget.value=""; if(input)void read(input);
  }}/>
    <button className="button compact" disabled={busy} title="현재 구간 자막을 ASS 파일로 교체합니다. 실행 취소할 수 있습니다."
      onClick={()=>{target.current=scene;setError("");file.current?.click();}}>ASS 불러오기</button>
    {error && <span role="alert">{error}</span>}</>;
}

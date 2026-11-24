import { useEffect, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { invoke, type EditorSeek, type ProjectSession } from "./projectSession";
import { useCutroomEvent } from "./useCutroomEvent";
import "./ProjectHome.css";

type RecentProject = { id: string; title: string | null; path: string };
type OpenProject = {type: "browse"} | {type: "recent"; id: string} | {type: "create"; name: string};

export function Brand() {
  return <><span className="brand-mark" aria-hidden="true"><i /><i /></span><strong>컷룸</strong></>;
}

export function ProjectHome({session, resume}: {session: ProjectSession; resume: (seek?: EditorSeek) => void}) {
  const [projects, setProjects] = useState<RecentProject[] | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  const [working, setWorking] = useState(false);
  const switching = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    let current = true;
    invoke<RecentProject[]>("project_recent").then(items => {
      if (current) setProjects([...items.filter(item => item.id === session.id), ...items.filter(item => item.id !== session.id)]);
    }).catch(e => { if (current) setError(String(e)); });
    return () => { current = false; };
  }, [refresh, session.id]);

  useCutroomEvent<string>("cutroom-selection-request", nonce => {
    void emit("cutroom-selection-" + nonce, {session: session.session, projectId: session.id,
      previewMode: "home", sceneId: null, sourceId: null, clip: null, selectedTrack: null,
      timelinePlaying: false, playhead: null});
  });
  useCutroomEvent<EditorSeek>("cutroom-seek", target => {
    if (!switching.current && target.session === session.session) resume(target);
  });

  async function open(action: OpenProject) {
    if (switching.current) return;
    switching.current = true;
    setWorking(true);
    setError("");
    try {
      if (action.type === "recent" && action.id === session.id) {
        resume();
        return;
      }
      if (await invoke<boolean>("project_switch", {action})) {
        // A new document binds every editor command to the newly opened folder.
        window.location.replace("?view=editor");
        return;
      }
    } catch (e) { setError(String(e)); }
    switching.current = false;
    setWorking(false);
  }

  function newProject() {
    setError("");
    setName("");
    dialog.current?.showModal();
  }

  return <main className="project-home">
    <header className="home-header"><div className="brand"><Brand /></div><span>프로젝트 홈</span></header>
    <div className="home-content">
      <div className="home-title">
        <h1>프로젝트</h1>
        <div className="home-actions">
          <button className="button" disabled={working} onClick={() => void open({type:"browse"})}>기존 폴더 열기</button>
          <button className="button primary" disabled={working} onClick={newProject}><span aria-hidden="true">＋</span> 새 프로젝트</button>
        </div>
      </div>
      {session.notice && <p className="home-notice" role="status">{session.notice}</p>}
      <section aria-labelledby="recent-heading">
        <h2 id="recent-heading">최근 프로젝트</h2>
        {projects === null && !error && <p role="status">프로젝트 목록을 불러오는 중…</p>}
        {projects?.length === 0 && <p>최근에 연 프로젝트가 없습니다.</p>}
        <ul className="home-projects">{projects?.map(project => {
          const active = project.id === session.id && project.title !== null;
          return <li key={project.id}><button className={"home-project" + (active ? " resume-project" : "")}
            disabled={working || project.title === null} onClick={() => void open({type:"recent", id:project.id})}>
            <span className="home-folder" aria-hidden="true" />
            <span className="home-project-info"><strong>{project.title ?? "열 수 없는 프로젝트"}</strong><small>{project.path}</small></span>
            <span className="home-open">{active ? "이어서 편집" : project.title === null ? "폴더 확인 필요" : "열기"}{project.title !== null && <span aria-hidden="true"> →</span>}</span>
          </button></li>;
        })}</ul>
      </section>
      {error && !dialog.current?.open && <p className="home-error" role="alert">{error} {projects === null && <button className="button compact" onClick={() => {setError(""); setRefresh(n => n + 1);}}>다시 시도</button>}</p>}
    </div>
    <dialog ref={dialog} className="new-project" aria-labelledby="new-title" onClose={() => setError("")} onCancel={event => { if (working) event.preventDefault(); }}>
      <form onSubmit={event => {event.preventDefault(); if (name.trim()) void open({type:"create", name:name.trim()});}}>
        <h2 id="new-title">새 프로젝트</h2>
        <label htmlFor="project-name">프로젝트 이름</label>
        <input id="project-name" autoFocus value={name} disabled={working} onChange={event => setName(event.target.value)} />
        {error && <p className="home-error" role="alert">{error}</p>}
        <footer><button className="button" type="button" disabled={working} onClick={() => dialog.current?.close()}>취소</button>
          <button className="button primary" disabled={working || !name.trim()}>저장 위치 선택</button></footer>
      </form>
    </dialog>
  </main>;
}


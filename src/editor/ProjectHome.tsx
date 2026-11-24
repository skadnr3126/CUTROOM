import { useEffect, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { invoke, type EditorSeek, type ProjectSession } from "./projectSession";
import { useCutroomEvent } from "./useCutroomEvent";
import "./ProjectHome.css";

type RecentProject = { id: string; title: string | null; path: string };
type PackagePreview = {
  path: string; header: {version: number; id: string; title: string; editedMs: number};
  existing: {id: string; path: string; editedMs: number} | null;
};
type OpenProject = {type: "browse" | "openFile"} | {type: "recent"; id: string} | {type: "create"; name: string};

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
  const conflict = useRef<HTMLDialogElement>(null);
  const [incoming, setIncoming] = useState<PackagePreview | null>(null);
  const [status, setStatus] = useState("");

  useEffect(() => {
    if (incoming) conflict.current?.showModal();
    else conflict.current?.close();
  }, [incoming]);

  async function transfer(message: string, action: () => Promise<void>) {
    if (switching.current) return;
    switching.current = true;
    setWorking(true);
    setError("");
    setStatus(message);
    try { await action(); }
    catch (e) { setError(String(e)); setStatus(""); }
    finally { switching.current = false; setWorking(false); }
  }

  async function importPackage(preview: PackagePreview, mode: "new" | "replace" | "copy") {
    setIncoming(null);
    setStatus("프로젝트 패키지를 가져오는 중…");
    if (await invoke<boolean>("project_import_package", {preview, mode})) window.location.replace("?view=editor");
    else setStatus("");
  }

  function pickPackage() {
    void transfer("가져올 패키지를 선택하세요.", async () => {
      const preview = await invoke<PackagePreview | null>("project_pick_package");
      setStatus("");
      if (preview?.existing) setIncoming(preview);
      else if (preview) await importPackage(preview, "new");
    });
  }

  function exportPackage(id: string) {
    void transfer("프로젝트 패키지를 내보내는 중…", async () => {
      const path = await invoke<string | null>("project_export_package", {id});
      setStatus(path ? `내보내기 완료 · ${path}` : "");
    });
  }

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

  async function forget(id: string) {
    if (switching.current) return;
    switching.current = true;
    setWorking(true);
    setError("");
    try {
      await invoke("project_forget", {id});
      setProjects(items => items?.filter(item => item.id !== id) ?? null);
    } catch (e) { setError(String(e)); }
    finally { switching.current = false; setWorking(false); }
  }

  return <main className="project-home">
    <header className="home-header"><div className="brand"><Brand /></div><span>프로젝트 홈</span></header>
    <div className="home-content">
      <div className="home-title">
        <h1>프로젝트</h1>
        <div className="home-actions">
          <button className="button" disabled={working} onClick={pickPackage}>패키지 가져오기</button>
          <button className="button primary" disabled={working} title="컷룸 프로젝트의 project.json 파일을 선택합니다."
            onClick={() => void open({type:"openFile"})}>기존 프로젝트 열기</button>
          <button className="button" disabled={working} onClick={newProject}><span aria-hidden="true">＋</span> 새 프로젝트</button>
        </div>
      </div>
      {session.notice && <p className="home-notice" role="status">{session.notice}</p>}
      {status && <p className="home-notice" role="status">{status}</p>}
      <section aria-labelledby="recent-heading">
        <div className="home-recent-header"><h2 id="recent-heading">최근 프로젝트</h2>
          <button className="text-button" disabled={working} onClick={() => void open({type:"browse"})}>기존 폴더 열기</button>
        </div>
        {projects === null && !error && <p role="status">프로젝트 목록을 불러오는 중…</p>}
        {projects?.length === 0 && <p>최근에 연 프로젝트가 없습니다.</p>}
        <ul className="home-projects">{projects?.map(project => {
          const active = project.id === session.id && project.title !== null;
          return <li key={project.id}><button className={"home-project" + (active ? " resume-project" : "")}
            disabled={working || project.title === null} onClick={() => void open({type:"recent", id:project.id})}>
            <span className="home-folder" aria-hidden="true" />
            <span className="home-project-info"><strong>{project.title ?? "열 수 없는 프로젝트"}</strong><small>{project.path}</small></span>
            <span className="home-open">{active ? "이어서 편집" : project.title === null ? "폴더 확인 필요" : "열기"}{project.title !== null && <span aria-hidden="true"> →</span>}</span>
          </button><button className="text-button forget-project" disabled={working || project.title === null}
            aria-label={`${project.title} 패키지 내보내기`} onClick={() => exportPackage(project.id)}>패키지 내보내기</button>
          <button className="text-button forget-project" disabled={working}
            aria-label={`${project.title ?? project.path} 최근 목록에서 제거`}
            title="홈 목록에서만 제거합니다. 프로젝트 폴더는 보관됩니다."
            onClick={() => void forget(project.id)}>목록에서 제거</button></li>;
        })}</ul>
      </section>
      {error && !dialog.current?.open && <p className="home-error" role="alert">{error} {projects === null && <button className="button compact" onClick={() => {setError(""); setRefresh(n => n + 1);}}>다시 시도</button>}</p>}
    </div>
    <dialog ref={conflict} className="new-project" aria-labelledby="conflict-title" onCancel={() => setIncoming(null)}>
      {incoming?.existing && <>
        <h2 id="conflict-title">같은 프로젝트가 있습니다</h2>
        <p><strong>{incoming.header.title}</strong></p>
        <p>가져올 버전 · {new Date(incoming.header.editedMs).toLocaleString("ko-KR")}<br />
          기존 버전 · {new Date(incoming.existing.editedMs).toLocaleString("ko-KR")}</p>
        <p className="home-notice">교체하면 홈에서 가져온 버전이 열립니다. 기존 폴더는 백업으로 보관합니다.<br />{incoming.existing.path}</p>
        <footer><button className="button" onClick={() => setIncoming(null)}>취소</button>
          <button className="button" onClick={() => void transfer("", () => importPackage(incoming, "copy"))}>별도 복사본</button>
          <button className="button primary" onClick={() => void transfer("", () => importPackage(incoming, "replace"))}>백업 후 교체</button></footer>
      </>}
    </dialog>
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


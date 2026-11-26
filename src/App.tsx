import { captureExternalDrop, type ExternalDrop } from "./editor/dropTree";
import { useEffect, useRef, useState } from "react";
import { useSceneRename } from "./editor/SceneTitleInput";
import { ScriptPanel, type ScriptJump } from "./editor/ScriptPanel";
import { Stage } from "./editor/Stage";
import { CaptureControls } from "./editor/CaptureControls";
import { EditorLayout } from "./editor/EditorLayout";
import { emptyProject, mediaTrack, sceneClips, sceneLabel, sceneSettings, withClips, useProject, videoDuration, type MediaTrack, type Project, type Scene, type SceneSettings } from "./editor/project";
import { useTimelineUndoKey } from "./editor/useTimelineUndoKey";
import { useImagePaste } from "./editor/useImagePaste";
import { Brand, ProjectHome } from "./editor/ProjectHome";
import { openSession, type EditorSeek, type ProjectSession } from "./editor/projectSession";
import { useScriptDraft } from "./editor/scriptDraft";
import "./App.css";

type Draft = { revision: number; text: string; notes: string };

function savedWriting(scene: Scene): Draft {
  return { revision: scene.revision, text: scene.text, notes: scene.notes ?? "" };
}

export default function App() {
  const [session,setSession] = useState<ProjectSession | null>(null);
  const [error,setError] = useState("");
  const [editing, setEditing] = useState(() => new URLSearchParams(location.search).get("view") === "editor");
  const [initialSeek, setInitialSeek] = useState<EditorSeek>();
  useEffect(() => { void openSession().then(setSession).catch(e => setError(String(e))); }, []);
  if (!session) return <main role="status">{error || "프로젝트 여는 중…"}{error && <button onClick={() => location.reload()}>다시 열기</button>}</main>;
  if (!editing) return <ProjectHome session={session} resume={seek => {setInitialSeek(seek); setEditing(true);}} />;
  return <Editor session={session} initialSeek={initialSeek}/>;
}

function Editor({session, initialSeek}: {session:ProjectSession; initialSeek?:EditorSeek}) {
  const [leaving, setLeaving] = useState(false);
  const [captureBusy,setCaptureBusy] = useState(false);
  const { view, task, relinkSource, notice, setNotice, load, edit, importMedia, captureRegion, saveRecording, readScriptFile, exportMp4, saveClip } = useProject();
  const [sceneId, setSceneId] = useState<string | null>(null);
  const [recordingTrack, setRecordingTrack] = useState<MediaStreamTrack | null>(null);
  const [folderView, setFolderView] = useState<string | null>(null);
  const [sourceId, setSourceId] = useState<string | null>(null);
  const [mode, setMode] = useState<"timeline" | "source">("timeline");
  const [selectionVersion, bumpSelection] = useState(0);
  const [timelineJump, setTimelineJump] = useState<ScriptJump | null>(null);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const confirmingRemoval = useRef(false);
  const project = view?.project ?? emptyProject;
  const folders = project.folders ?? [];
  const activeFolder = folders.some(f => f.id === folderView) ? folderView : null;
  const script = useScriptDraft(project.scenes, session);
  const scene = project.scenes.find((entry) => entry.id === sceneId) ?? project.scenes[0] ?? null;
  const source = project.assets.find((entry) => entry.id === sourceId && !entry.removedFromBin) ?? null;
  const activity = [task, recordingTrack && "화면 녹화 중"].filter(Boolean).join(" · ");
  useEffect(() => { if (session.notice) setNotice({text:session.notice}); }, [session]);
  const busy = !!task || !view || leaving;
  const draft = scene ? draftFor(scene) ?? savedWriting(scene) : null;
  const dirty = !!scene && !!draftFor(scene);

  async function goHome() {
    if (leaving || task || captureBusy) return;
    if (project.scenes.some(target => draftFor(target))) {
      setNotice({text:"수정 중인 구간 대본을 저장하거나 취소한 뒤 홈으로 돌아가 주세요."});
      return;
    }
    setLeaving(true);
    try {
      await script.flush();
      window.location.replace(window.location.pathname);
    } catch (error) {
      setLeaving(false);
      setNotice({text:String(error), error:true});
    }
  }

  function draftFor(target: Scene) {
    const pending = drafts[target.id];
    return pending?.revision === target.revision ? pending : undefined;
  }

  function chooseScene(id: string, showEditor = true) {
    setSceneId(id);
    setMode("timeline");
    if (showEditor) bumpSelection((version) => version + 1);
  }

  function chooseSource(id: string) {
    setSourceId(id);
    setMode("source");
  }

  const rename = useSceneRename(async ({ id, original, value }) => {
    const target = project.scenes.find(s => s.id === id);
    if (!target || (target.name ?? "") !== original) {
      setNotice({ text: "구간 이름이 변경되었습니다. Esc를 누르고 새 이름을 확인해 주세요." });
      return false;
    }
    const next = await edit({ type: "renameScene", id, expected: target.revision, name: value });
    if (!next) return false;
    rebaseTimelineDrafts(next);
    return true;
  });

  async function importSources(files?: File[] | ExternalDrop, folderId: string | null = null) {
    const existing = new Set(project.assets.map(asset => asset.id));
    const next = await importMedia(files, folderId);
    return next?.assets.filter(asset => !existing.has(asset.id)).map(asset => asset.id) ?? [];
  }

  async function importToBin(files?: File[] | ExternalDrop, folderId = activeFolder) {
    const [first] = await importSources(files, folderId);
    if (first) chooseSource(first);
  }

  useImagePaste(images => {
    if (busy) setNotice({ text: "현재 작업이 끝난 뒤 이미지를 다시 붙여넣어 주세요." });
    else void importToBin(images);
  });

  function updateDraft(field: "text" | "notes", value: string) {
    if (!scene || busy) return;
    setDrafts(current => {
      const saved = savedWriting(scene);
      const existing = current[scene.id];
      const updated = { ...(existing?.revision === scene.revision ? existing : saved), [field]: value };
      const next = { ...current };
      if (updated.text === saved.text && updated.notes === saved.notes) delete next[scene.id];
      else next[scene.id] = updated;
      return next;
    });
  }

  function forgetDraft(id: string) {
    setDrafts((current) => {
      const next = { ...current };
      delete next[id];
      return next;
    });
  }

  function showAddedScene(next: Project | null, showEditor = true) {
    const added = next?.scenes.find((candidate) => !project.scenes.some((old) => old.id === candidate.id));
    if (added) chooseScene(added.id, showEditor);
    return added?.id ?? null;
  }

  async function saveScene(target: Scene, settings: SceneSettings) {
    const next = await edit({ type: "updateScene", id: target.id, expected: target.revision, settings });
    if (next) {
      forgetDraft(target.id);
      chooseScene(target.id);
    }
  }

  async function moveScene(id: string, targetId: string, after: boolean) {
    const next = await edit({ type: "moveScene", id, targetId, after });
    if (next) chooseScene(id);
  }

  async function changePlacement(id: string, track: MediaTrack, assetId: string | null, clipId?: string) {
    const target = project.scenes.find((entry) => entry.id === id);
    if (!target || !view || busy) return;
    const asset = project.assets.find((candidate) => candidate.id === assetId);
    const settings = sceneSettings(target);
    if (track === "audio") {
      await storePlacement(target, { ...settings, audio: assetId ? { assetId, inMs: 0 } : null });
      return;
    }
    const clips = sceneClips(target);
    if (asset && asset.kind !== "audio") {
      const durationMs = asset.kind === "video" ? Math.min(await videoDuration(view.mediaDir, asset) ?? target.durationMs, 86_400_000)
        : clips.length ? 3000 : target.durationMs;
      await storePlacement(target, withClips(settings, [...clips, {
        id: crypto.randomUUID(), assetId: asset.id, inMs: 0, durationMs,
        trim: asset.kind === "video" ? { offsetMs: 0, outMs: durationMs } : undefined,
      }]));
    } else if (!assetId && clipId) {
      await storePlacement(target, withClips(settings, clips.filter((clip) => clip.id !== clipId)));
    }
  }

  async function storePlacement(target: Scene, settings: SceneSettings) {
    const { id } = target;
    const next = await edit({ type: "updateScene", id, expected: target.revision, settings });
    const saved = next?.scenes.find((entry) => entry.id === id);
    if (!saved) return;
    rebaseDraft(target, saved);
    setMode("timeline");
  }

  function rebaseDraft(before: Scene, after: Scene) {
    setDrafts(current => {
      const pending = current[before.id];
      if (pending?.revision !== before.revision) return current;
      const next = { ...current };
      const text = pending.text === before.text ? after.text : pending.text;
      const notes = pending.notes === (before.notes ?? "") ? after.notes ?? "" : pending.notes;
      if (text === after.text && notes === (after.notes ?? "")) delete next[before.id];
      else next[before.id] = { revision: after.revision, text, notes };
      return next;
    });
  }

  function rebaseTimelineDrafts(next: Project) {
    for (const after of next.scenes) {
      const before = project.scenes.find(s => s.id === after.id);
      if (before && before.revision !== after.revision) rebaseDraft(before, after);
    }
  }

  async function undoTimeline() {
    const target = view?.undo;
    if (!target || busy || confirmingRemoval.current) return;
    const before = project.scenes.find(entry => entry.id === target.sceneId);
    if (target.removesScene && before && draftFor(before)) {
      setNotice({ text: "이 구간에 작성 중인 대본을 먼저 저장해 주세요." });
      return;
    }
    const next = await edit({ type: "undo", token: target.token });
    if (!next) return;
    const restored = next.scenes.find(entry => entry.id === target.sceneId);
    rebaseTimelineDrafts(next);
    if (restored) chooseScene(restored.id, scene?.id !== restored.id);
    else if (before) forgetDraft(before.id);
  }

  useTimelineUndoKey(undoTimeline);

  async function removeScene(target: Scene) {
    if (busy || confirmingRemoval.current) return;
    confirmingRemoval.current = true;
    try {
      const index = project.scenes.findIndex((entry) => entry.id === target.id);
      if (!(await window.confirm(`‘${sceneLabel(target, index)}’ 구간과 타임라인 배치를 삭제할까요?`))) return;
      const next = await edit({ type: "removeScene", id: target.id, expected: target.revision });
      if (next) {
        forgetDraft(target.id);
        const neighbor = next.scenes[Math.min(index, next.scenes.length - 1)];
        setSceneId((current) => (current ?? project.scenes[0]?.id) === target.id ? neighbor?.id ?? null : current);
      }
    } catch {
      setNotice({ text: "구간 삭제 확인창을 열 수 없습니다.", error: true });
    } finally {
      confirmingRemoval.current = false;
    }
  }

  return (
    <main className="studio" onDragOver={(event) => {
      if (!event.dataTransfer.types.includes("Files")) return;
      event.preventDefault();
      event.stopPropagation();
      event.dataTransfer.dropEffect = busy ? "none" : "copy";
    }} onDrop={(event) => {
      if (!event.dataTransfer.types.includes("Files")) return;
      event.preventDefault();
      event.stopPropagation();
      if (busy) setNotice({ text: "현재 작업이 끝난 뒤 파일을 다시 놓아 주세요." });
      else void importToBin(captureExternalDrop(event.dataTransfer));
    }}>
      <header className="studio-header">
        <button className="brand" aria-label="컷룸 프로젝트 홈" title="프로젝트 홈으로 돌아가기" disabled={leaving || !!task || captureBusy} onClick={() => void goHome()}><Brand /></button>
        <div className="project-heading">
          <span>프로젝트</span>
          <form onSubmit={(event) => { event.preventDefault(); event.currentTarget.querySelector("input")?.blur(); }}>
            <input key={project.title} aria-label="프로젝트 이름" defaultValue={project.title} disabled={busy} onBlur={(event) => {
              const title = event.currentTarget.value.trim();
              if (title && title !== project.title) {
                const field = event.currentTarget;
                void edit({ type: "rename", title }).then((next) => { if (!next) field.value = project.title; });
              } else {
                event.currentTarget.value = project.title;
              }
            }} />
          </form>
        </div>
        <div className="studio-actions">
          <span className={"local-state" + (activity ? " working" : "")} role="status" aria-live="polite" aria-atomic="true">
            <i aria-hidden="true" />{activity || (view ? "이 기기에 저장" : "프로젝트 열기")}
          </span>
          <button className="button compact" disabled={busy || !view?.undo} aria-keyshortcuts="Control+Z"
            title="마지막 타임라인 편집 되돌리기 (Ctrl+Z)" onClick={() => void undoTimeline()}>되돌리기</button>
          <button className="button export-button primary" disabled={busy || !project.scenes.length || !view?.exportAvailable}
            title={view?.exportAvailable ? "영상·자막·음성이 합쳐진 MP4 저장" : "FFmpeg를 설치하고 앱을 다시 열어 주세요."}
            onClick={() => void exportMp4()}>MP4 내보내기 <span aria-hidden="true">↗</span></button>
        </div>
      </header>
      {notice && <div className={`studio-notice ${notice.error ? "error" : ""}`} role="status">
        <span>{notice.text}</span>
        {!view && <button className="button compact" onClick={() => void load()}>다시 열기</button>}
        <button className="text-button" aria-label="알림 닫기" onClick={() => setNotice(null)}>×</button>
      </div>}
      <EditorLayout>
        <Stage initialSeek={initialSeek} folderControls={{ folders, active: activeFolder, open: setFolderView, edit: async action => !!await edit(action),
          importFiles: (files, id) => void importToBin(files, id) }} recordingTrack={recordingTrack} project={project} mediaDir={view?.mediaDir ?? ""} scene={scene} selectionVersion={selectionVersion} source={source} mode={mode} onMode={setMode}
          onBgmChange={clips => { void edit({type:"setBgm", expected:project.bgm ?? [], clips}); }}
          captureControls={<CaptureControls onActivity={setCaptureBusy} onPreview={setRecordingTrack} busy={busy} ffmpegAvailable={!!view?.exportAvailable} onSave={id => saveRecording(id, activeFolder)}
            onCapture={() => void captureRegion(activeFolder).then(id => { if (id) chooseSource(id); })} />}
          onChooseScene={(id) => { chooseScene(id, false); setTimelineJump({ sceneId: id }); }} onChooseSource={chooseSource} busy={busy}
          onChangePlacement={changePlacement}
          onRemoveScene={removeScene} onMoveScene={moveScene}
          sceneRename={rename}
          onSaveClip={(id, revision, clipId) => void saveClip(id, revision, clipId, activeFolder).then((saved) => { if (saved) chooseSource(saved); })}
          onSceneTool={async (target, tool) => {
            const next = await edit({ type: "sceneTool", sceneId: target.id, expected: target.revision, tool });
            if (next) rebaseTimelineDrafts(next);
            return next?.scenes.find(s=>s.id===target.id)?.revision ?? null;
          }}
          onVisualChange={storePlacement}
          onCutRange={async (target, clipId, startMs, endMs) => {
            const next = await edit({ type: "cutClipRange", folderId: activeFolder, sceneId: target.id, expected: target.revision, range: { clipId, startMs, endMs } });
            if (next) {
              rebaseTimelineDrafts(next);
              setNotice({ text: "잘라낸 영상을 소스 보관함에 저장했습니다." });
            }
          }}
          onRenameSource={async (id, name) => !!await edit({ type: "renameSource", id, name })}
          onRelinkSource={relinkSource}
          onRemoveSource={(id) => void edit({ type: "removeSource", id }).then((next) => {
            if (next && sourceId === id) { setSourceId(null); setMode("timeline"); }
          })}
          onImport={() => void importToBin()}
          onLink={() => {
            if (scene && source) void changePlacement(scene.id, mediaTrack(source), source.id);
          }} />
        <ScriptPanel script={script} scenes={project.scenes} scene={scene} draftText={draft?.text ?? ""} draftNotes={draft?.notes ?? ""} dirty={dirty} busy={busy}
          timelineJump={timelineJump} onChoose={chooseScene} rename={rename}
          onCreate={() => void edit({ type: "addScene" }).then(showAddedScene)}
          onReadText={readScriptFile}
          onAddSelection={async (text) => showAddedScene(await edit({ type: "addScene", text }), false)}
          onEdit={updateDraft}
          onSave={() => { if (scene && draft) void saveScene(scene, { ...sceneSettings(scene), text: draft.text, notes: draft.notes }); }}
          onMove={moveScene}
          onRemove={removeScene} />
      </EditorLayout>
    </main>
  );
}

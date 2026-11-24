import { useCutroomEvent } from "./useCutroomEvent";
import { convertFileSrc } from "@tauri-apps/api/core";
import { invoke } from "./projectSession";
import { useEffect, useRef, useState } from "react";
import { readDropTree, type ExternalDrop } from "./dropTree";
import { importDroppedMedia } from "./mediaDrop";

export type SourceFolder = { id: string; parentId?: string; name: string };
export type FolderEdit =
  | { type: "createFolder"; id: string; name: string; parentId: string | null }
  | { type: "moveFolder"; id: string; parentId: string | null }
  | { type: "renameFolder"; id: string; name: string }
  | { type: "removeFolder"; id: string }
  | { type: "moveSources"; ids: string[]; folderId: string | null };

export type Asset = {
  projectFile?: string;
  missing?: boolean;
  folderId?: string;
  id: string;
  name: string;
  extension: string;
  kind: "video" | "image" | "audio";
  durationMs: number | null;
  hasAudio: boolean;
  sourceJobId?: string;
  removedFromBin?: boolean;
};

export type Placement = { assetId: string; inMs: number };
export type VideoTrim = { offsetMs: number; outMs: number };
export type VisualPlacement = Placement & { frame?: ClipFrame; muted?: boolean; trim?: VideoTrim };
export type ClipFrame = { size: number; x: number; y: number };
export type SceneClip = VisualPlacement & { gapMs?: number; id: string; durationMs: number };
export type Overlay = Placement & { track?: number; sound?: boolean; id: string; startMs: number; durationMs: number; size: number; x: number; y: number };
export type MediaTrack = "visual" | "audio";

export function mediaTrack(asset: Asset): MediaTrack {
  return asset.kind === "audio" ? "audio" : "visual";
}

export type CaptionCue = { startMs: number; endMs: number; text: string };

export type SceneSettings = {
  text: string;
  notes?: string;
  name?: string;
  caption: string;
  captionCues?: CaptionCue[];
  captionAss?: { name: string; source: string };
  durationMs: number;
  visual: VisualPlacement | null;
  clips?: SceneClip[];
  overlays?: Overlay[];
  audio: Placement | null;
};

export type Scene = SceneSettings & { id: string; revision: number };
export type GuidelineStyle = { id: string; name: string; text: string; referenceIds: string[] };
export type Project = { folders?: SourceFolder[]; timelineJobs?: string[]; title: string; guidelineStyles: GuidelineStyle[]; selectedStyleId: string | null; guidelinesRevision: number; scenes: Scene[]; assets: Asset[] };
type View = { mediaExtensions: string[]; project: Project; mediaDir: string; exportAvailable: boolean;
  undo: { token: string; sceneId: string | null; removesScene: boolean } | null };

export type SceneTool = { type: "deleteCaption"; startMs: number; endMs: number } | { type: "addCaption"; atMs: number; text: string } | { type: "editAssRows"; edits: (CaptionCue & {line:number})[] } | { type: "removeAssTrack" } | { type: "setAssTrack"; name: string; source: string } | { type: "setCaptionCues"; cues: CaptionCue[] } | { type: "duplicateClip"; clipId: string };

export type Edit =
  | { type: "sceneTool"; sceneId: string; expected: number; tool: SceneTool }
  | FolderEdit
  | { type: "cutClipRange"; folderId: string | null; sceneId: string; expected: number; range: { clipId: string; startMs: number; endMs: number } }
  | { type: "undo"; token: string }
  | { type: "renameSource"; id: string; name: string }
  | { type: "removeSource"; id: string }
  | { type: "rename"; title: string }
  | { type: "addScene"; text?: string }
  | { type: "renameScene"; id: string; expected: number; name: string }
  | { type: "updateScene"; id: string; expected: number; settings: SceneSettings; titleCard?: string }
  | { type: "moveScene"; id: string; targetId: string; after: boolean }
  | { type: "removeScene"; id: string; expected: number };

export const assetLabels = { video: "영상", image: "이미지", audio: "음성" };
export const sourceDragType = "application/x-cutroom-asset";
export const emptyProject: Project = { title: "새 영상 프로젝트", guidelineStyles: [], selectedStyleId: null, guidelinesRevision: 0, scenes: [], assets: [] };

export function sourceUrl(directory: string, asset: Asset) {
  const projectDir = directory.replace(/[\\/\\\\]media[\\/\\\\]?$/i, "");
  if (asset.missing) return convertFileSrc(projectDir + '/.cutroom-offline/' + asset.id);
  return convertFileSrc(asset.projectFile ? `${projectDir}/${asset.projectFile}` : `${directory}/${asset.id}.${asset.extension}`);
}

export async function videoDuration(directory: string, asset: Asset): Promise<number | null> {
  if (asset.durationMs != null) return asset.durationMs;
  const video = document.createElement("video");
  try {
    return await new Promise<number | null>((resolve) => {
      const finish = (duration: number | null) => {
        window.clearTimeout(timeout);
        resolve(duration);
      };
      const timeout = window.setTimeout(() => finish(null), 5000);
      video.onloadedmetadata = () => finish(Number.isFinite(video.duration) && video.duration > 0 ? Math.round(video.duration * 1000) : null);
      video.onerror = () => finish(null);
      video.preload = "metadata";
      video.src = sourceUrl(directory, asset);
    });
  } finally {
    video.onerror = video.onloadedmetadata = null;
    video.removeAttribute("src");
    video.load();
  }
}

export function timelineTime(ms: number) {
  const total = Math.max(0, Math.floor(ms / 1_000));
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

export function sceneLabel(scene: Scene, index: number) {
  return scene.name?.trim() || `구간 ${index + 1}`;
}

export function sceneSettings(scene: Scene): SceneSettings {
  const { text, notes, name, caption, captionCues, captionAss, durationMs, visual, audio, clips, overlays } = scene;
  return { text, notes, name, caption, captionCues, captionAss, durationMs, visual, audio, clips, overlays };
}

export function sceneClips(scene: Scene): SceneClip[] {
  if (scene.clips?.length) return scene.clips;
  return scene.visual ? [{ ...scene.visual, id: scene.id, durationMs: scene.durationMs }] : [];
}

export function withClips(settings: SceneSettings, clips: SceneClip[]): SceneSettings {
  return { ...settings, visual: null, clips, durationMs: clips.length
    ? clips.reduce((sum, clip) => sum + (clip.gapMs ?? 0) + clip.durationMs, 0) : settings.durationMs };
}

export function useProject() {
  const [view, setView] = useState<View | null>(null);
  const [task, setTask] = useState<string | null>("프로젝트 여는 중");
  const [notice, setNotice] = useState<{ text: string; error?: boolean } | null>(null);
  const working = useRef(false);
  const currentTask = useRef<Promise<void> | null>(null);

  async function run<T>(label: NonNullable<typeof task>, request: () => Promise<T>) {
    if (working.current) return null;
    working.current = true;
    let complete!: () => void;
    currentTask.current = new Promise<void>(resolve => { complete = resolve; });
    setTask(label);
    setNotice(null);
    try {
      return await request();
    } catch (error) {
      setNotice({ text: typeof error === "string" ? error : error instanceof Error ? error.message : "프로젝트 작업을 완료할 수 없습니다.", error: true });
      return null;
    } finally {
      working.current = false;
      currentTask.current = null;
      complete();
      setTask(null);
    }
  }

  async function load() {
    const result = await run("프로젝트 여는 중", () => invoke<View>("project_load"));
    if (result) setView(result);
  }

  useEffect(() => { void load(); }, []);
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    async function sync() {
      try {
        if (!working.current && document.visibilityState === "visible") {
          const changed = await invoke<View | null>("project_sync_media");
          if (changed && alive) {
            while (currentTask.current) await currentTask.current;
            if (alive) await load();
          }
        }
      } catch (e) { if (alive) setNotice({text:String(e), error:true}); }
      if (alive) timer = setTimeout(sync, 2500);
    }
    timer = setTimeout(sync, 2500);
    return () => { alive = false; clearTimeout(timer); };
  }, []);
  useCutroomEvent("cutroom-project-changed", async () => {
    while (currentTask.current) await currentTask.current;
    await load();
  });


  async function edit(edit: Edit) {
    const label = edit.type === "cutClipRange" ? "잘라낸 영상 저장 중"
      : edit.type === "sceneTool" ? (edit.tool.type === "duplicateClip" ? "클립 복제 중" : "자막 저장 중")
      : "프로젝트 저장 중";
    const result = await run(label, () => invoke<View>("project_edit", { edit }));
    if (result) setView(result);
    return result?.project ?? null;
  }

  async function relinkSource(assetId:string) {
    const next = await run("소스 다시 연결 중", () => invoke<View | null>("project_relink_media", {assetId}));
    if (next) setView(next);
  }

  async function importMedia(input?: File[] | ExternalDrop, folderId: string | null = null) {
    const result = await run("소스 가져오는 중", async () => {
      if (!input) return invoke<View | null>("project_import_media", { folderId });
      if (Array.isArray(input)) return importDroppedMedia<View>(input, undefined, folderId);
      if (!view) throw new Error("프로젝트를 연 뒤 다시 가져와 주세요.");
      const batch = await readDropTree(input, view.mediaExtensions);
      const next = await importDroppedMedia<View>(batch, undefined, folderId);
      if (batch.folders.length) {
        const skipped = batch.skipped ? ` · 지원하지 않는 파일 ${batch.skipped}개 제외` : "";
        setNotice({ text: `폴더 ${batch.folders.length}개와 소스 ${batch.files.length}개를 가져왔습니다.${skipped}` });
      }
      return next;
    });
    if (result) setView(result);
    return result?.project ?? null;
  }

  async function captureRegion(folderId: string | null = null) {
    const saved = await run("캡처 영역 선택 및 가져오는 중", () =>
      invoke<{ view: View; assetId: string } | null>("capture_region", { folderId }));
    if (!saved) return null;
    setView(saved.view);
    setNotice({ text: "선택한 영역을 소스 보관함에 추가했습니다." });
    return saved.assetId;
  }

  async function saveRecording(id: string, folderId: string | null = null) {
    // Stopping always releases the screen immediately, even during an editor operation.
    while (currentTask.current) await currentTask.current;
    const saved = await run("녹화 MP4 저장 중", () =>
      invoke<{ view: View; assetId: string }>("finish_recording", { id, folderId }));
    if (!saved) return false;
    setView(saved.view);
    setNotice({ text: "화면 녹화 MP4를 소스 보관함에 추가했습니다." });
    return true;
  }

  async function exportMp4() {
    const saved = await run("MP4 내보내는 중", () => invoke<boolean>("project_export"));
    if (saved) setNotice({ text: "MP4 편집본을 저장했습니다." });
  }

  function saveClip(sceneId: string, expected: number, clipId: string, folderId: string | null = null) {
    return addRenderedSource("project_save_clip", { sceneId, expected, clipId, folderId });
  }

  async function addRenderedSource(command: string, args: Record<string, unknown>) {
    const result = await run("편집 영상 만드는 중", () => invoke<{ view: View; assetId: string }>(command, args));
    if (!result) return null;
    setView(result.view);
    setNotice({ text: "편집본을 소스 보관함에 추가했습니다." });
    return result.assetId;
  }

  function readScriptFile() {
    return run("대본 가져오는 중", () => invoke<string | null>("read_script_file"));
  }

  return { view, task, relinkSource, notice, setNotice, load, edit, importMedia, captureRegion, saveRecording, readScriptFile, exportMp4, saveClip };
}

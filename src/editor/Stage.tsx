import { emit } from "@tauri-apps/api/event";
import { openSession, type EditorSeek } from "./projectSession";
import { useCutroomEvent } from "./useCutroomEvent";
import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { sceneLabel, mediaTrack, type ClipEdit, type SceneSettings, type SceneTool, sourceDragType, sourceUrl, timelineTime, videoDuration, type Asset, type MediaTrack, type Project, type Scene } from "./project";
import { CaptionTrack } from "./CaptionTrack";
import { AssCaptionTrack } from "./AssCaptionTrack";
import { sceneCaptions, captionAt, type CaptionDraft } from "./captionCues";
import { AddCaptionDialog } from "./AddCaptionDialog";
import { AssEditor, useAssRows } from "./AssEditor";
import { adjacentCaptionStarts, useCaptionStarts } from "./useCaptionStarts";
import { AssPreview } from "./AssPreview";
import { SeamlessVideo, type PreviewVideoSource } from "./SeamlessVideo";
import { TimelineImage } from "./TimelineImage";
import { AssFileButton } from "./AssFileButton";
import { CaptionEditor } from "./CaptionEditor";
import { VisualTrack } from "./VisualTrack";
import { NeighborPreviewTrack } from "./NeighborPreviewTrack";
import { CutRangeControls } from "./CutRangeControls";
import { visualClips, trackSettings, trackNumbers, canSplit, resizeClip, removeClip, placeClip, endOf, fullFrame, frameStyle, type TrackClip } from "./visualTracks";
import { timelineSections, type TimelinePiece } from "./timeline";
import type { FolderControls } from "./BinFolders";
import { SourceBin } from "./SourceBin";
import { StageLayout } from "./EditorLayout";
import type { SceneRename } from "./SceneTitleInput";
import { TimelineOverview } from "./TimelineOverview";
import { anchorTime } from "./bgm";
import type { BgmClip, BgmSourceRange } from "./project";
import { TimelineRuler } from "./TimelineRuler";
import { useTimelineZoom } from "./useTimelineZoom";
import { ClipMenu, type MenuAction } from "./ClipMenu";
import { sourceTime, videoRange } from "./videoTrim";
import { overlayPlaybackRuns } from "./overlayPlaybackRuns";

type Track = MediaTrack | "captions";

const transitionSideMs = 2000;
const previewPrerollMs = 2000;
const previewRates = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3];

type Props = {
  onBgmChange: (clips: BgmClip[]) => void;
  initialSeek?: EditorSeek;
  folderControls: FolderControls;
  recordingTrack: MediaStreamTrack | null;
  onSceneTool: (scene: Scene, tool: SceneTool) => Promise<number|null>;
  onVisualChange: (scene: Scene, settings: SceneSettings) => Promise<void>;
  onClipEdit: (scene: Scene, operation: ClipEdit) => Promise<boolean>;
  captureControls: import("react").ReactNode;
  project: Project;
  mediaDir: string;
  scene: Scene | null;
  selectionVersion: number;
  source: Asset | null;
  mode: "timeline" | "source";
  onMode: (value: Props["mode"]) => void;
  onChooseScene: (id: string) => void;
  onChooseSource: (id: string) => void;
  onImport: () => void;
  onLink: () => void;
  onChangePlacement: (sceneId: string, track: MediaTrack, assetId: string | null, clipId?: string) => void;
  onSaveClip: (id: string, revision: number, clipId: string) => void;
  onRenameSource: (id: string, name: string) => Promise<boolean>;
  onRelinkSource: (id:string) => void;
  onRemoveSource: (id: string) => void;
  onRemoveScene: (target: Scene) => void;
  sceneRename: SceneRename;
  onMoveScene: (id: string, targetId: string, after: boolean) => void;
  busy: boolean;
};

export function Stage({ initialSeek, folderControls, recordingTrack, onSceneTool, onVisualChange, onClipEdit, captureControls, project, onBgmChange, mediaDir, scene, selectionVersion, source, mode, onMode, onChooseScene, onChooseSource, onImport, onLink, onChangePlacement, onSaveClip, onRenameSource, onRelinkSource, onRemoveSource, onRemoveScene, onMoveScene, sceneRename, busy }: Props) {
  const [watchRecording, setWatchRecording] = useState(true);
  const sourceMedia = useRef<HTMLMediaElement | null>(null);
  useEffect(() => { setWatchRecording(true); }, [recordingTrack]);
  const liveTrack = watchRecording ? recordingTrack : null;
  const [clipId, setClipId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Scene | null>(null);
  const working = draft && draft.id === scene?.id && draft.revision === scene.revision ? draft : scene;
  const sections = useMemo(() => timelineSections(working && working !== scene ? project.scenes.map(s => s.id === working.id ? working : s) : project.scenes),
    [project.scenes, working, scene]);
  const section = sections.find(entry => entry.scene.id === scene?.id);
  const spans = useMemo(() => sections.flatMap(entry => entry.pieces), [sections]);
  const detail = section?.pieces ?? [];
  const duration = sections[sections.length - 1]?.end ?? 0;
  const selectedStart = section?.start ?? 0;
  const items = useMemo(() => working ? visualClips(working) : [],[working]);
  const lanes = trackNumbers(items);
  const newTrack = lanes[0] + 1;
  const selected = items.find(c => c.id === clipId);
  function changeItems(next: TrackClip[], commit = true) {
    if (!scene || busy) return;
    const settings = trackSettings(scene,next);
    if (commit) { setDraft(null); void onVisualChange(scene,settings); }
    else setDraft({...scene,...settings});
  }
  function selectItem(clip: TrackClip, at: number, reveal = true) {
    if (scene && reveal) onChooseScene(scene.id);
    setClipId(clip.id); setPicked(null); setPlaying(false); onMode("timeline"); seek(selectedStart+at);
  }
  async function dropItem(track: number, at: number, item: Asset | TrackClip, target?: string, after = false) {
    if (!scene || busy) return;
    let clip: TrackClip;
    if ("kind" in item) {
      if (item.kind === "audio") return;
      const sourceMs = item.kind === "video" ? await videoDuration(mediaDir,item) : 3000;
      if (item.kind === "video" && sourceMs != null && item.durationMs == null) setMeasured(old => ({...old,[item.id]:sourceMs}));
      const durationMs = Math.min(sourceMs ?? (item.kind === "video" ? scene.durationMs : 3000),86_400_000);
      clip = {id:crypto.randomUUID(),assetId:item.id,inMs:0,track,startMs:0,durationMs,
        trim: item.kind === "video" ? {offsetMs:0,outMs:durationMs} : undefined};
    } else {
      clip = item;
      // Upper tracks store only the visible source window of a padded legacy video.
      if (!clip.track && track && byId.get(clip.assetId)?.kind === "video") {
        const range = videoRange(clip,clip.durationMs,byId.get(clip.assetId)?.durationMs ?? measured[clip.assetId]);
        clip = {...clip,durationMs:range.outMs-range.inMs};
      }
    }
    finishDrag();
    const next = placeClip(items,clip,track,at,target,after);
    const placed = next.find(c => c.id === clip.id);
    if (placed && next !== items) { changeItems(next); selectItem(placed,placed.startMs); }
  }
  const [scope, setScope] = useState<"all" | "detail">("all");
  const [showTransitions, setShowTransitions] = useState(false);
  const [previewRate, setPreviewRate] = useState(1);
  const [boundaryRun, setBoundaryRun] = useState<{ end: number } | null>(null);
  const timelineStart = scope === "detail" ? selectedStart : 0;
  const timelineEnd = scope === "detail" ? section?.end ?? 0 : duration;
  const previous = section && sections[section.index - 1];
  const following = section && sections[section.index + 1];
  const transitionMode = scope === "detail" && showTransitions && !!section;
  const playbackStart = transitionMode && previous ? Math.max(previous.start, selectedStart - transitionSideMs) : timelineStart;
  const playbackEnd = transitionMode && following ? Math.min(following.end, timelineEnd + transitionSideMs) : timelineEnd;
  const requestedPosition = useRef<{ id: string; time: number } | null>(null);
  const [position, setPosition] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [picked, setPicked] = useState<{ sceneId: string; track: Track } | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const positionRef = useRef(0);
  const seekVersion = useRef(0);
  const byId = useMemo(() => new Map(project.assets.map((asset) => [asset.id, asset])), [project.assets]);
  const binAssets = useMemo(() => project.assets.filter((asset) => !asset.removedFromBin), [project.assets]);
  const [menu, setMenu] = useState<MenuAction | null>(null);
  const labelWidth = scope === "detail" ? 63 : 0;
  const scopeDuration = playbackEnd - playbackStart;
  const zoom = useTimelineZoom(scope, scopeDuration, Math.max(0, Math.min(scopeDuration, position - playbackStart)), labelWidth);
  const { pixelsPerMs, viewport, scrollRef } = zoom;
  const leadingWidth = (timelineStart - playbackStart) * pixelsPerMs;
  const followingLeft = (timelineEnd - playbackStart) * pixelsPerMs;
  const editViewport = { left: viewport.left - leadingWidth, right: viewport.right - leadingWidth };
  const dragAsset = dragId ? byId.get(dragId) : undefined;
  const draggingVisual = !busy && !!dragAsset && dragAsset.kind !== "audio";
  const [measured, setMeasured] = useState<Record<string, number>>({});

  function clipRange(target: TimelinePiece) {
    if (!target.visual) return null;
    const asset = byId.get(target.visual.assetId);
    return asset?.kind === "video" ? videoRange(target.visual, target.durationMs, asset.durationMs ?? measured[asset.id]) : null;
  }


  function seek(value: number) {
    seekVersion.current++;
    movePlayhead(value);
  }

  function movePlayhead(value: number) {
    positionRef.current = value;
    setPosition(value);
  }

  useEffect(() => {
    setPlaying(false);
    const request = requestedPosition.current;
    requestedPosition.current = null;
    seek(request && request.id === scene?.id ? request.time : selectedStart);
  }, [scene?.id, selectedStart, selectionVersion]);

  // Returning to a zoomed timeline should reveal the selected edit point.
  const detailSceneId = scope === "detail" ? scene?.id : undefined;
  function revealTime(absoluteMs: number) {
    const scroller = scrollRef.current;
    if (!scroller) return;
    const x = Math.max(0, absoluteMs - playbackStart) * pixelsPerMs;
    const width = scroller.clientWidth - labelWidth;
    if (x < scroller.scrollLeft || x >= scroller.scrollLeft + width) {
      scroller.scrollLeft = Math.max(0, x - width / 2);
    }
  }
  useEffect(() => { revealTime(positionRef.current); }, [scope, detailSceneId, selectionVersion, transitionMode]);

  function selectOverview(id: string, at: number) {
    setWatchRecording(false);
    requestedPosition.current = id === scene?.id ? null : { id, time: at };
    setPlaying(false);
    setPicked(null);
    onChooseScene(id);
    seek(at);
    onMode("timeline");
  }

  const pendingSeek = useRef(initialSeek);
  useEffect(() => {
    const request = pendingSeek.current;
    if (!request) return;
    const target = sections.find(entry => entry.scene.id === request.sceneId);
    if (target) {
      pendingSeek.current = undefined;
      selectOverview(request.sceneId, target.start + request.atMs);
    }
  }, [sections]);

  useCutroomEvent<EditorSeek>("cutroom-seek", async ({sceneId, atMs, session}) => {
    if ((await openSession()).session !== session) return;
    const target = sections.find(entry => entry.scene.id === sceneId);
    if (target) selectOverview(sceneId, target.start + atMs);
  });

  function switchScope(next: typeof scope) {
    setScope(next);
    setPlaying(false);
    onMode("timeline");
    if (next === "detail" && section && (position < section.start || position >= section.end)) seek(section.start);
  }

  function toggleTransitions() {
    setPlaying(false);
    setBoundaryRun(null);
    if (showTransitions) seek(Math.max(timelineStart, Math.min(positionRef.current, timelineEnd)));
    setShowTransitions(!showTransitions);
  }

  function playBoundary(edge: "start" | "end") {
    if (!section) return;
    const before = edge === "start";
    seek(before ? playbackStart : Math.max(section.start, section.end - transitionSideMs));
    setBoundaryRun({ end: before ? Math.min(section.end, section.start + transitionSideMs) : playbackEnd });
    setWatchRecording(false);
    onMode("timeline");
    setPlaying(true);
  }

  function goToSection(offset: number) {
    const target = section && sections[section.index + offset];
    if (!target) return;
    setClipId(null);
    setDraft(null);
    setMenu(null);
    selectOverview(target.scene.id, target.start);
  }

  useEffect(() => {
    if (!playing || mode !== "timeline") return;
    const initial = positionRef.current;
    const started = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      const end = boundaryRun ? Math.min(boundaryRun.end - 1, playbackEnd) : playbackEnd;
      const next = Math.min(end, initial + (now - started) * previewRate);
      movePlayhead(next);
      if (next < end) frame = requestAnimationFrame(tick);
      else setPlaying(false);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playbackEnd, mode, playing, previewRate, boundaryRun]);

  const time = Math.max(playbackStart, Math.min(position, playbackEnd));
  const bgmRanges = useMemo(() => (project.bgm ?? []).map(clip => ({clip, start:anchorTime(clip.start,sections), end:anchorTime(clip.end,sections)})), [project.bgm,sections]);
  const visibleSpans = scope === "detail" && !transitionMode ? detail : spans;
  const currentIndex = visibleSpans.findIndex((span) => Math.min(time, playbackEnd - 1) < span.end);
  const current = visibleSpans[currentIndex] ?? visibleSpans[visibleSpans.length - 1];
  const currentScene = current?.scene;
  const overlayRuns = useMemo(() => overlayPlaybackRuns(sections), [sections]);
  const overlays = current && overlayRuns.get(current.owner.id);
  const [assDraft, setAssDraft] = useState<{ scene: Scene; at: number; line?: number } | null>(null);
  const [assPreview, setAssPreview] = useState<string>("");
  const assTarget = assDraft && sections.find(s => s.scene.id === assDraft.scene.id);
  const assRows = useAssRows(section?.scene.captionAss);
  const captionStarts = useCaptionStarts(sections, playbackStart, playbackEnd);
  const adjacent = adjacentCaptionStarts(captionStarts, time);
  function jumpToCaption(target: number | undefined) {
    if (target === undefined) return;
    scrubTo(target);
    revealTime(target);
    scrollRef.current?.querySelector<HTMLInputElement>(".ruler-seek")?.focus({preventScroll:true});
  }
  const activeAss = current?.owner.captionAss;
  function editAss(target: NonNullable<typeof section>, at: number, line?: number) {
    if (!target.scene.captionAss || busy) return;
    selectOverview(target.scene.id, target.start + at);
    setWatchRecording(false);
    setAssPreview(target.scene.captionAss.source);
    setAssDraft({scene:target.scene, at, line});
  }
  const [captionDraft, setCaptionDraft] = useState<CaptionDraft | null>(null);
  const [addingCaption, setAddingCaption] = useState<{scene: Scene; atMs: number} | null>(null);
  const addTarget = addingCaption && sections.find(s => s.scene.id === addingCaption.scene.id);
  const captionTarget = captionDraft && sections.find(s => s.scene.id === captionDraft.sceneId);
  const playheadSection = current && sections[current.index];
  const previewingNeighbor = transitionMode && playheadSection?.scene.id !== section?.scene.id;
  useCutroomEvent<string>("cutroom-selection-request", async nonce => {
    const binding = await openSession();
    const piece = picked && spans.find(entry => entry.scene.id === picked.sceneId);
    const track = piece && picked ? {sceneId:piece.owner.id, kind:picked.track,
      assetId:picked.track === "audio" ? piece.scene.audio?.assetId ?? (!piece.scene.visual?.muted && byId.get(piece.scene.visual?.assetId ?? "")?.hasAudio ? piece.scene.visual?.assetId : null) : null} : null;
    await emit("cutroom-selection-" + nonce, {
      session:binding.session, projectId:binding.id, sceneId:scene?.id ?? null, sourceId:source?.id ?? null,
      clip:selected ? {id:selected.id,assetId:selected.assetId,track:selected.track, startMs:selected.startMs,durationMs:selected.durationMs} : null,
      selectedTrack:track, previewMode:liveTrack ? "recording" : mode, timelinePlaying:playing,
      playhead:{timelineMs:Math.round(time),sceneId:playheadSection?.scene.id ?? null,
        atMs:playheadSection ? Math.round(time - playheadSection.start) : null}
    });
  });
  const previewCues = current && (captionDraft?.sceneId === current.owner.id ? captionDraft.cues : sceneCaptions(current.owner));
  const previewCaption = current && previewCues && captionAt(previewCues, Math.min(time, playheadSection!.end - 1) - current.sectionStart)?.text;
  function openCaption(target: typeof section, ms: number, index?: number) {
    if (!target || busy) return;
    if (index === undefined) {
      const atMs = Math.max(0, Math.min(target.durationMs - 1, Math.floor(ms)));
      setWatchRecording(false);
      selectOverview(target.scene.id, target.start + atMs);
      setScope("detail");
      setClipId(null);
      setAddingCaption({scene: target.scene, atMs});
      return;
    }
    const cues = sceneCaptions(target.scene);
    const entry = { cues, index };
    setWatchRecording(false);
    selectOverview(target.scene.id, target.start + entry.cues[entry.index].startMs);
    setScope("detail");
    setClipId(null);
    setCaptionDraft({ sceneId: target.scene.id, revision: target.scene.revision, ...entry });
  }
  function addAtPlayhead() {
    if (!previewingNeighbor && playheadSection && time < playheadSection.end) openCaption(playheadSection, time - playheadSection.start);
  }
  useEffect(() => {
    function shortcut(event: KeyboardEvent) {
      if (event.code === "Space") {
        if (event.defaultPrevented || event.isComposing || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey ||
            liveTrack || menu || document.querySelector("dialog[open]") || event.target instanceof Element &&
            event.target.closest('textarea, select, input:not([type="range"]), [contenteditable]:not([contenteditable="false"]), [role="textbox"], [role="dialog"], [role="menu"]')) return;
        const media = sourceMedia.current;
        if (mode === "source" ? !media : !duration) return;
        // Native media controls handle Space on keyup when they have focus.
        if (media && event.target === media) return;
        event.preventDefault();
        if (event.repeat) return;
        if (media && mode === "source") {
          if (media.paused) void media.play().catch(() => {});
          else media.pause();
        } else togglePlay();
        return;
      }
      if (event.code === "KeyA" || event.code === "KeyD") {
        if (event.defaultPrevented || event.isComposing || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey ||
            busy || liveTrack || mode !== "timeline" || menu || document.querySelector("dialog[open]") ||
            event.target instanceof Element && event.target.closest('textarea, select, input:not([type="range"]), [contenteditable]:not([contenteditable="false"])')) return;
        const targets = adjacentCaptionStarts(captionStarts, positionRef.current);
        event.preventDefault();
        jumpToCaption(event.code === "KeyA" ? targets.previous : targets.next);
        return;
      }
      if (event.key === "F2" && !event.defaultPrevented && !event.repeat && !event.isComposing &&
        !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey && !busy &&
        !captionDraft && !assDraft && !previewingNeighbor && scope === "detail" && playheadSection && mode === "timeline" &&
        (event.target === document.body || event.target instanceof Element && event.target.matches(".ruler-seek, .transport input[type=range]"))) {
        const ms = time - playheadSection.start;
        if (playheadSection.scene.captionAss) {
          const row = assRows?.rows.find(r => r.startMs <= ms && ms < r.endMs);
          if (row) { event.preventDefault(); editAss(playheadSection, ms, row.line); }
        } else {
          const index = sceneCaptions(playheadSection.scene).findIndex(c => c.startMs <= ms && ms < c.endMs);
          if (index >= 0) { event.preventDefault(); openCaption(playheadSection, ms, index); }
        }
        return;
      }
      if (!event.ctrlKey || !event.altKey || event.shiftKey || event.code !== "KeyC" || event.repeat ||
        event.isComposing || captionDraft || assDraft || addingCaption || event.target instanceof Element &&
        event.target.closest("input,textarea,select,dialog,[contenteditable=true]")) return;
      event.preventDefault();
      addAtPlayhead();
    }
    window.addEventListener("keydown", shortcut);
    return () => window.removeEventListener("keydown", shortcut);
  }, [playheadSection, time, busy, captionDraft, assDraft, addingCaption, assRows, scope, mode, previewingNeighbor,
    captionStarts, liveTrack, menu, pixelsPerMs, playbackStart, playbackEnd, playing, position, duration, onMode]);
  const hasPictureAbove = current?.owner.overlays?.some(c => {
    const t = Math.min(time-current.sectionStart,sections[current.index].durationMs-1);
    return t >= c.startMs && t < c.startMs+c.durationMs;
  });
  const at = Math.round(time-selectedStart);
  const splitTarget = selected ?? [...items].sort((a,b) => b.track-a.track).find(c => at >= c.startMs && at < endOf(c));
  const splitAsset = splitTarget && byId.get(splitTarget.assetId);
  const allowSplit = splitTarget && splitAsset && canSplit(splitTarget,at,splitAsset.kind === "video",splitAsset.durationMs ?? measured[splitAsset.id]);
  const peers = items.filter(c => c.track === selected?.track).sort((a,b) => a.startMs-b.startMs);
  const index = selected ? peers.findIndex(c => c.id === selected.id) : -1;
  const frame = selected?.frame ?? fullFrame;
  function setFrame(patch: Partial<typeof frame>) {
    changeItems(items.map(c => c.id === selected?.id ? {...c, frame:{...frame,...patch}} : c));
  }
  function scrubTo(absoluteMs: number) {
    setWatchRecording(false);
    setPlaying(false);
    onMode("timeline");
    seek(absoluteMs);
  }
  function scrubSection(offsetMs: number) {
    scrubTo(timelineStart + offsetMs);
  }
  const localTime = current ? Math.min(time - current.start, current.durationMs - 1) : 0;
  const visualPlacement = currentScene?.visual;
  const range = currentScene && visualPlacement?.trim ? clipRange(currentScene) : null;
  const sourceStart = range?.inMs ?? visualPlacement?.inMs ?? 0;
  const clipTime = localTime - (range?.offsetMs ?? 0) + sourceStart;
  const insideRange = !range || (localTime >= range.offsetMs && clipTime < range.outMs);
  const visual = visualPlacement && insideRange ? byId.get(visualPlacement.assetId) : undefined;
  const audio = currentScene?.audio && byId.get(currentScene.audio.assetId);

  function shouldPreroll(startMs: number) {
    return startMs < playbackEnd && startMs >= time && startMs - time <= previewPrerollMs * previewRate;
  }

  function measureUnknownDuration(asset: Asset) {
    if (asset.durationMs == null && measured[asset.id] == null) {
      return (ms: number) => setMeasured(old => ({ ...old, [asset.id]: ms }));
    }
  }

  let currentVideo: PreviewVideoSource | undefined;
  if (current && visual?.kind === "video" && visualPlacement) {
    currentVideo = {
      url: sourceUrl(mediaDir, visual),
      originMs: current.start + (range?.offsetMs ?? 0) - sourceStart,
      timeMs: clipTime,
      endMs: range?.outMs ?? sourceStart + current.durationMs,
      muted: !!audio || visualPlacement.muted,
      onDuration: measureUnknownDuration(visual),
    };
  }
  let nextVideo: PreviewVideoSource | undefined;
  let nextImageUrl: string | undefined;
  if (visual && current && currentIndex >= 0 && shouldPreroll(current.end)) {
    const followingPiece = visibleSpans[currentIndex + 1];
    const placement = followingPiece?.scene.visual;
    const asset = placement && byId.get(placement.assetId);
    if (visual.kind === "image" && asset?.kind === "image") {
      nextImageUrl = sourceUrl(mediaDir, asset);
    } else if (currentVideo && placement && asset?.kind === "video") {
      const nextRange = clipRange(followingPiece.scene);
      if (nextRange && nextRange.offsetMs === 0 && nextRange.outMs > nextRange.inMs) {
        nextVideo = {
          url: sourceUrl(mediaDir, asset),
          originMs: followingPiece.start - nextRange.inMs,
          timeMs: nextRange.inMs,
        };
      }
    }
  }

  function togglePlay() {
    if (!duration) return;
    setBoundaryRun(null);
    if (!playing && (position < playbackStart || position >= playbackEnd)) seek(playbackStart);
    setPlaying(!playing);
    onMode("timeline");
  }

  function pickClip(id: string, track: Track, start: number) {
    setClipId(null);
    setPicked({ sceneId: id, track });
    setPlaying(false);
    seek(start);
    if (scene) onChooseScene(scene.id);
    onMode("timeline");
  }

  function finishDrag() {
    setDragId(null);
  }

  const tracks = [
    { label: "음성", glyph: "♪", className: "audio", labelFor: (scene: TimelinePiece) => byId.get(scene.audio?.assetId ?? "")?.name ||
      (!scene.visual?.muted && byId.get(scene.visual?.assetId ?? "")?.hasAudio ? "원본 소리" : undefined) },
  ] as const;
  const trackWidth = scopeDuration * pixelsPerMs + (zoom.isFit ? 0 : Math.min(160, 2000 * pixelsPerMs));
  const scaleText = `${Number((pixelsPerMs * 1000).toPrecision(3))}px/초`;

  return (
    <>
      <StageLayout>
        <SourceBin folderControls={folderControls} binAssets={binAssets} mediaDir={mediaDir} source={source} captureControls={captureControls}
          hasScene={!!scene} busy={busy} onImport={onImport} onLink={onLink}
          onChooseSource={id => { setPlaying(false); onChooseSource(id); }}
          onMenu={setMenu} onStartDrag={setDragId} onEndDrag={finishDrag}
          onRenameSource={onRenameSource} onRelinkSource={onRelinkSource} onRemoveSource={onRemoveSource} />
        <section className="panel monitor" id="preview-pane" aria-labelledby="monitor-title">
          <header className="panel-header">
            <h2 id="monitor-title">미리보기</h2>
            {!liveTrack && <select className="preview-rate" aria-label="미리보기 재생 속도" title="미리보기 재생 속도"
              value={previewRate} onChange={e => setPreviewRate(Number(e.target.value))}>
              {previewRates.map(rate => <option key={rate} value={rate}>{rate}×</option>)}
            </select>}
            <div className="segmented">
              <button aria-pressed={!liveTrack && mode === "timeline"} onClick={() => { setWatchRecording(false); onMode("timeline"); }}>편집본</button>
              <button aria-pressed={!liveTrack && mode === "source"} disabled={!source} onClick={() => { setWatchRecording(false); setPlaying(false); onMode("source"); }}>소스</button>
              {recordingTrack && <button aria-pressed={!!liveTrack} onClick={() => setWatchRecording(true)}>녹화</button>}
            </div>
          </header>
          <div className="monitor-body">
            {liveTrack ? <RecordingMonitor track={liveTrack} /> : mode === "source" && source ? (
              <SourcePreview key={source.id} asset={source} mediaDir={mediaDir} rate={previewRate} media={sourceMedia} />
            ) : (
              <div className="picture composite-picture" aria-label="편집본 미리보기"><div className="composite-frame">
                {visual?.kind === "image" && current ? (
                  <TimelineImage url={sourceUrl(mediaDir, visual)} name={visual.name} frame={visualPlacement?.frame}
                    startMs={current.start} endMs={current.end} playing={playing} seekVersion={seekVersion.current} nextUrl={nextImageUrl} />
                ) : (
                  <div className="preview-layer" style={frameStyle(visualPlacement?.frame)}>
                    {currentVideo ? (
                      <SeamlessVideo current={currentVideo} next={nextVideo} playing={playing} rate={previewRate} />
                    ) : (
                      !hasPictureAbove && <div className="picture-empty"><span aria-hidden="true">▷</span>{range ? "이 위치에는 영상 클립이 없습니다" : current ? "이 구간에 영상 소스를 넣어 보세요" : "대본을 추가하고 편집을 시작하세요"}</div>
                    )}
                  </div>
                )}
                {bgmRanges.map(({clip,start,end}) => {
                  const asset = byId.get(clip.assetId);
                  if (!asset || time < start || time >= end) return null;
                  const fadeIn = clip.fadeInMs ? Math.min(1,(time-start)/Math.min(end-start,clip.fadeInMs)) : 1;
                  const fadeOut = clip.fadeOutMs ? Math.min(1,(end-time)/Math.min(end-start,clip.fadeOutMs)) : 1;
                  return <SyncedAudio key={clip.id+":"+asset.id} url={sourceUrl(mediaDir,asset)} loop sourceRange={clip.sourceRange} time={(time-start)/1000}
                    playing={playing} rate={previewRate} volume={clip.volume/100*fadeIn*fadeOut}/>;
                })}
                {audio && <SyncedAudio key={`${current?.owner.id}:${audio.id}`} url={sourceUrl(mediaDir, audio)}
                  time={(localTime + (current ? current.start - current.sectionStart : 0) + (currentScene?.audio?.inMs ?? 0)) / 1000} playing={playing} rate={previewRate} />}
                {current && overlays?.map(({ layer, key, start, next }) => {
                  const at = Math.min(time - current.sectionStart, sections[current.index].durationMs - 1) - layer.startMs;
                  const asset = byId.get(layer.assetId);
                  if (!asset || at < 0 || at >= layer.durationMs) return null;
                  const nextAsset = next && shouldPreroll(next.start) ? byId.get(next.layer.assetId) : undefined;
                  if (asset.kind === "image") {
                    return <TimelineImage key={key} url={sourceUrl(mediaDir, asset)} name={asset.name} frame={layer}
                      startMs={start} endMs={start + layer.durationMs} playing={playing} seekVersion={seekVersion.current}
                      nextUrl={nextAsset?.kind === "image" ? sourceUrl(mediaDir, nextAsset) : undefined} />;
                  }
                  return (
                    <div className="preview-layer" key={key} style={frameStyle(layer)}>
                      <SeamlessVideo
                        current={{
                          url: sourceUrl(mediaDir, asset),
                          originMs: start - layer.inMs,
                          timeMs: at + layer.inMs,
                          endMs: layer.inMs + layer.durationMs,
                          muted: !layer.sound || !!audio,
                          onDuration: measureUnknownDuration(asset),
                        }}
                        next={next && nextAsset?.kind === "video" ? {
                          url: sourceUrl(mediaDir, nextAsset), originMs: next.start - next.layer.inMs, timeMs: next.layer.inMs,
                        } : undefined}
                        playing={playing}
                        rate={previewRate}
                      />
                    </div>
                  );
                })}
                {activeAss && current && <AssPreview source={assDraft?.scene.id===current.owner.id ? assPreview : activeAss.source} ms={Math.max(0,time-current.sectionStart)}/>}
                {!activeAss && previewCaption?.trim() && <div className="caption-overlay"><span>{previewCaption}</span></div>}
              </div></div>
            )}
          </div>
          <footer className="transport" hidden={!!liveTrack}>
            <output className="timecode">{timelineTime(time - playbackStart)} <span>/ {timelineTime(playbackEnd - playbackStart)}</span></output>
            {mode === "timeline" && <button className="button compact" disabled={busy || adjacent.previous === undefined}
              title="이전 자막 시작 (A)" aria-label="이전 자막 시작" aria-keyshortcuts="A" onClick={() => jumpToCaption(adjacent.previous)}>◀ A</button>}
            <button className="round-play" disabled={!duration} onClick={togglePlay} aria-keyshortcuts={mode==="timeline"?"Space":undefined}
              title={mode==="timeline"?"재생 / 일시 정지 (Space)":"타임라인 재생"} aria-label={playing ? "타임라인 일시 정지" : "타임라인 재생"}>{playing ? "Ⅱ" : "▶"}</button>
            {mode === "timeline" && <button className="button compact" disabled={busy || adjacent.next === undefined}
              title="다음 자막 시작 (D)" aria-label="다음 자막 시작" aria-keyshortcuts="D" onClick={() => jumpToCaption(adjacent.next)}>D ▶</button>}
            <input
              type="range"
              min={0}
              max={Math.max(1, playbackEnd - playbackStart)}
              step={10}
              value={time - playbackStart}
              aria-label="타임라인 재생 위치"
              disabled={!duration}
              onChange={(event) => scrubTo(playbackStart + event.target.valueAsNumber)}
            />
          </footer>
        </section>
      </StageLayout>

      <section className="panel timeline-panel" id="studio-timeline" aria-label="타임라인">
        <header className="panel-header timeline-header">
          <div className="segmented timeline-toggle" role="group" aria-label="타임라인 보기">
            <button aria-pressed={scope === "all"} onClick={() => switchScope("all")}>전체 보기</button>
            <button aria-pressed={scope === "detail"} disabled={!section} onClick={() => switchScope("detail")}>상세 보기</button>
          </div>
          {scope === "detail" && (previous || following) && <button className="button compact transition-toggle"
            aria-pressed={showTransitions} onClick={toggleTransitions}>이음새 미리보기</button>}
          <button className="button compact" disabled={previewingNeighbor || !playheadSection || time >= playheadSection.end || busy}
            title="재생 위치에 자막 추가 (Ctrl+Alt+C). 기존 자막 중간에서는 그 위치부터 새 문구를 입력합니다."
            aria-keyshortcuts="Control+Alt+C" onClick={addAtPlayhead}>＋ 자막 추가</button>
          {playheadSection && <AssFileButton scene={playheadSection.scene} busy={busy || previewingNeighbor} save={async(s,t)=>(await onSceneTool(s,t))!==null}/>}
          {playheadSection?.scene.captionAss && <button className="button compact" disabled={busy || previewingNeighbor} onClick={()=>editAss(playheadSection,time-playheadSection.start)}>ASS 자막 편집</button>}
          {playheadSection && !playheadSection.scene.captionAss && <button className="button compact" disabled={busy || previewingNeighbor || !sceneCaptions(playheadSection.scene).length}
            onClick={() => {
              const cues = sceneCaptions(playheadSection.scene);
              const ms = time - playheadSection.start;
              const next = cues.findIndex(c => c.endMs > ms);
              const index = next < 0 ? cues.length - 1 : next;
              openCaption(playheadSection, cues[index].startMs, index);
            }}>자막 편집</button>}
          {scope === "all" ? <span className="panel-meta">{sections.length}개 구간 · {timelineTime(duration)}</span> : <div className="timeline-actions">
            <button className="button compact" disabled={busy || !section || section.index === 0} onClick={() => goToSection(-1)}>이전 구간</button>
            <span className="panel-meta">{section ? sceneLabel(section.scene, section.index) : "구간 –"}</span>
            <button className="button compact" disabled={busy || !section || section.index + 1 >= sections.length} onClick={() => goToSection(1)}>다음 구간</button>
            <div className="clip-tools" role="group" aria-label="클립 편집 도구">
            <button className="button compact" disabled={busy || !allowSplit} title="재생바 위치에서 클립을 둘로 나누고 양쪽을 모두 유지합니다." onClick={() => {
              if (allowSplit && scene) {
                void onClipEdit(scene, { type: "splitClip", clipId: splitTarget.id, atMs: at - splitTarget.startMs });
              }
            }}>클립 나누기</button>
            <button className="button compact" disabled={busy || !selected || !scene} onClick={() => {
              if (scene && selected) void onSceneTool(scene, { type: "duplicateClip", clipId: selected.id });
            }}>클립 복제</button>
            {scene && section && selected &&
              <CutRangeControls key={scene.id + ":" + scene.revision + ":" + selected.track}
                track={selected.track} clips={peers} durationMs={section.durationMs} playhead={at} busy={busy}
                sourceRange={clip => {
                  const asset = byId.get(clip.assetId);
                  return asset?.kind === "video" ? videoRange(clip, clip.durationMs, asset.durationMs ?? measured[asset.id]) : undefined;
                }}
                pause={() => setPlaying(false)} cut={async (operation, startMs) => {
                  setPlaying(false);
                  const endPosition = positionRef.current;
                  const saved = await onClipEdit(scene, operation);
                  if (saved && positionRef.current === endPosition) scrubSection(startMs);
                }} />}
            <button className="button compact" disabled={busy || (!selected && !(picked?.track === "audio" && scene?.audio))}
              onClick={() => { if (selected) changeItems(removeClip(items,selected)); else if (scene) onChangePlacement(scene.id,"audio",null); }}>
              {picked?.track === "audio" && !selected ? "음성 연결 해제" : "클립 삭제"} <kbd>Delete</kbd></button>
            </div>
            {source && source.kind !== "audio" && scene && <button className="button compact" disabled={busy || previewingNeighbor}
              onClick={() => void dropItem(newTrack,at,source)}>선택 소스를 새 트랙에 추가</button>}
            {selected && <>
              <label>크기 <select aria-label="클립 크기" disabled={busy} value={frame.size} onChange={e => setFrame({size:Number(e.target.value)})}>
                {[...new Set([25,40,50,65,80,100,frame.size])].sort((a,b)=>a-b).map(n=><option key={n} value={n}>{n}%</option>)}</select></label>
              <select aria-label="클립 위치" disabled={busy} value={frame.x+","+frame.y} onChange={e => {const [x,y]=e.target.value.split(",").map(Number); setFrame({x,y});}}>
                {[["50,50","중앙"],["0,0","왼쪽 위"],["100,0","오른쪽 위"],["0,100","왼쪽 아래"],["100,100","오른쪽 아래"]].map(([v,l])=><option key={v} value={v}>{l}</option>)}</select>
              {byId.get(selected.assetId)?.kind === "video" && <span className="panel-meta">소스 {sourceTime(selected.inMs)}–{sourceTime(videoRange(selected,selected.durationMs,byId.get(selected.assetId)?.durationMs ?? measured[selected.assetId]).outMs)}</span>}
              {byId.get(selected.assetId)?.hasAudio && <button className="button compact" aria-pressed={!selected.muted} disabled={busy}
                onClick={() => changeItems(items.map(c => c.id === selected.id ? {...c,muted:!c.muted} : c))}>
                {selected.muted ? "원본 소리 꺼짐" : "원본 소리 켜짐"}</button>}
              {byId.get(selected.assetId)?.kind === "image" && <label>표시 시간 <input aria-label="이미지 표시 시간(초)" type="number" min="0.1" step="0.1"
                key={selected.id+":"+selected.durationMs} disabled={busy} defaultValue={selected.durationMs/1000}
                onKeyDown={e=>{if(e.key === "Enter") e.currentTarget.blur();}} onBlur={e=>{
                  const ms = Math.round(e.target.valueAsNumber*1000);
                  if (ms >= 100 && ms <= 86_400_000 && ms !== selected.durationMs)
                    changeItems(resizeClip(items,selected,"end",ms-selected.durationMs,false));
                  else e.target.value=String(selected.durationMs/1000);
                }}/> 초</label>}
              {([-1,1] as const).map(d=><button key={d} className="button compact" disabled={busy || !peers[index+d]}
                aria-label={d<0 ? "클립을 앞으로 이동" : "클립을 뒤로 이동"} onClick={() => void dropItem(selected.track,0,selected,peers[index+d].id,d>0)}>{d<0 ? "←" : "→"}</button>)}
            </>}
          </div>}
          <div className="timeline-zoom" role="group" aria-label="타임라인 확대 및 축소">
            <button type="button" className="button compact" disabled={!scopeDuration || zoom.sliderValue <= 0}
              aria-label="타임라인 축소" title="타임라인 축소" onClick={() => zoom.setSliderValue(Math.max(0, zoom.sliderValue - 5))}>−</button>
            <input type="range" min={0} max={100} step={0.25} value={zoom.sliderValue} disabled={!scopeDuration}
              aria-label="타임라인 가로 배율" aria-valuetext={scaleText} title={scaleText}
              onChange={event => zoom.setSliderValue(event.target.valueAsNumber)} />
            <button type="button" className="button compact" disabled={!scopeDuration || zoom.sliderValue >= 100}
              aria-label="타임라인 확대" title="타임라인 확대" onClick={() => zoom.setSliderValue(Math.min(100, zoom.sliderValue + 5))}>＋</button>
            <button type="button" className="button compact" disabled={!scopeDuration} aria-pressed={zoom.isFit}
              onClick={zoom.fit}>화면에 맞춤</button>
          </div>
        </header>
        {transitionMode && (previous || following) && <div className="transition-preview" role="group" aria-label="이음새 재생">
          {previous && <button className="button compact" title={sceneLabel(previous.scene, previous.index)} onClick={() => playBoundary("start")}>▶ 앞 이음새</button>}
          {following && <button className="button compact" title={sceneLabel(following.scene, following.index)} onClick={() => playBoundary("end")}>▶ 뒤 이음새</button>}
          {playheadSection && <span className="panel-meta">미리보기 중 · {sceneLabel(playheadSection.scene, playheadSection.index)}</span>}
        </div>}
        {scope === "all" ? <TimelineOverview sections={sections} assets={byId} mediaDir={mediaDir} selectedId={scene?.id}
          bgm={project.bgm ?? []} onBgmChange={clips => {setWatchRecording(false); onMode("timeline"); onBgmChange(clips);}}
          pixelsPerMs={pixelsPerMs} viewport={viewport} scrollRef={scrollRef} onSeek={scrubSection}
          time={time} playing={playing} busy={busy} dragAsset={dragAsset} select={selectOverview} onImport={onImport}
          onOpenDetail={(id, at) => { selectOverview(id, at); setScope("detail"); }}
          onRemove={onRemoveScene} rename={sceneRename} onMenu={setMenu} onMove={onMoveScene}
          add={(id, asset) => { onChangePlacement(id, mediaTrack(asset), asset.id); finishDrag(); }} /> :
        <div className="timeline-scroll" ref={scrollRef}>
          <div className="timeline-grid" data-dragging-visual={draggingVisual} style={{ minWidth: labelWidth + trackWidth }}>
            <div className="track-labels"><span className="ruler-label">시간</span>{section && [newTrack,...lanes].map((n,i)=><span className="visual-track-label" data-new-track={n === newTrack} key={n} aria-label={i ? "트랙 "+(n+1) : "새 트랙"}><b>{i ? "▤" : "＋"}</b>{i ? "V"+(n+1) : "트랙"}</span>)}<span className="captions-track-label"><b>CC</b>자막</span>{tracks.map((track) => <span className={track.className+"-track-label"} key={track.label}><b>{track.glyph}</b>{track.label}</span>)}</div>
            <div className="track-content">
              <TimelineRuler pixelsPerMs={pixelsPerMs} viewport={viewport} duration={scopeDuration} time={time - playbackStart}
                onSeek={ms => scrubTo(playbackStart + ms)} label="상세 타임라인 재생바" />
              <div className="detail-track-bands">
              {transitionMode && previous && <NeighborPreviewTrack side="앞" section={previous} start={playbackStart} end={selectedStart}
                time={time} scale={pixelsPerMs} assets={byId} mediaDir={mediaDir} viewport={viewport} seek={scrubTo} />}
              <div className="detail-edit-tracks" style={transitionMode && (previous || following) ? { flex: "none", width: (timelineEnd - timelineStart) * pixelsPerMs } : undefined}>
              {section && [newTrack,...lanes].map(n => <VisualTrack key={n} track={n} fresh={n === newTrack} clips={items}
                selected={clipId} sceneId={section.scene.id} assets={byId} mediaDir={mediaDir} scale={pixelsPerMs} viewport={editViewport}
                busy={busy} draggingVisual={draggingVisual} onClipDrag={setDragId} measured={measured} select={selectItem} remove={c => changeItems(removeClip(items,c))}
                cancel={() => setDraft(null)} drop={dropItem}
                save={(c,x,y) => setMenu({x,y,items:[
                  ...(byId.get(c.assetId)?.kind === "video" ? [{label:"클립을 소스 보관함에 추가",run:()=>onSaveClip(section.scene.id,section.scene.revision,c.id)}] : []),
                  {label:"클립 복제",run:()=>{void onSceneTool(section.scene,{type:"duplicateClip",clipId:c.id});}},
                  {label:"클립 삭제",run:()=>changeItems(removeClip(items,c))}]})}
                resize={(c,edge,delta,commit) => {
                  const asset = byId.get(c.assetId);
                  if (!asset) return;
                  const next = resizeClip(items,c,edge,delta,asset.kind === "video",asset.durationMs ?? measured[asset.id]);
                  changeItems(next,commit);
                  const resized = next.find(item => item.id === c.id)!;
                  selectItem(c,edge === "start" ? resized.startMs : endOf(resized)-1,false);
                }} />)}
              {section?.scene.captionAss ? <AssCaptionTrack key={section.scene.id + ":" + section.scene.revision}
                rows={assRows?.rows ?? []} durationMs={section.scene.durationMs} scale={pixelsPerMs} busy={busy}
                seek={ms => { setWatchRecording(false); setClipId(null); setPicked(null); scrubSection(ms); }}
                edit={(ms,line) => editAss(section,ms,line)}
                menu={setMenu} remove={({startMs,endMs}) => { void onSceneTool(section.scene,{type:"deleteCaption",startMs,endMs}); }}
                save={edits => { void onSceneTool(section.scene,{type:"editAssRows",edits}); }} /> :
                scene && section ? <CaptionTrack key={scene.id + ":" + scene.revision} cues={sceneCaptions(scene)} durationMs={scene.durationMs}
                allowAdd scale={pixelsPerMs} busy={busy}
                menu={setMenu} remove={({startMs,endMs}) => { void onSceneTool(scene,{type:"deleteCaption",startMs,endMs}); }}
                open={(ms, index) => openCaption(section, ms, index)}
                seek={ms => { setWatchRecording(false); setClipId(null); setPicked(null); scrubSection(ms); }}
                save={cues => { void onSceneTool(scene, { type: "setCaptionCues", cues }); }} /> : <div className="track-row captions" />}
              {tracks.map(track => <div className={"track-row "+track.className} key={track.className}
                onDragOver={e => { if (!busy && track.className === "audio" && dragAsset?.kind === "audio") e.preventDefault(); }}
                onDrop={e => { if (!busy && scene && track.className === "audio" && dragAsset?.kind === "audio" && e.dataTransfer.getData(sourceDragType) === dragAsset.id) {
                  e.preventDefault(); onChangePlacement(scene.id,"audio",dragAsset.id); finishDrag();
                } }}>
                {detail.map(({scene:piece,start,durationMs},i) => {
                  const whole = !!section?.scene.audio;
                  if (whole && i) return null;
                  const label = track.labelFor(piece);
                  const active = picked?.sceneId === piece.id && picked.track === track.className;
                  return <div className="track-slot" key={piece.id} style={{width:(whole ? section!.durationMs : durationMs)*pixelsPerMs}}>
                    <button className={"clip "+(label ? "filled" : "gap")+(active ? " picked" : "")} onClick={()=>pickClip(piece.id,track.className,start)}
                      onKeyDown={e => { if (!busy && e.key === "Delete" && !e.repeat && section?.scene.audio && track.className === "audio") {
                        e.preventDefault(); onChangePlacement(section.scene.id,"audio",null);
                      } }}>{label || "–"}</button></div>;
                })}
              </div>)}
              </div>
              {transitionMode && following && <NeighborPreviewTrack side="뒤" section={following} start={timelineEnd} end={playbackEnd}
                time={time} scale={pixelsPerMs} assets={byId} mediaDir={mediaDir}
                viewport={{ left: viewport.left - followingLeft, right: viewport.right - followingLeft }} seek={scrubTo} />}
              </div>
              {section && <div className="play-line" style={{ left: (time - playbackStart) * pixelsPerMs }} aria-hidden="true" />}
              {!duration && <p className="timeline-hint">대본 구간을 추가하면 트랙이 만들어집니다.</p>}
            </div>
          </div>
        </div>}
      </section>
      {addingCaption && addTarget && <AddCaptionDialog atMs={addingCaption.atMs} stale={addTarget.scene.revision !== addingCaption.scene.revision}
        close={() => setAddingCaption(null)} save={async text => (await onSceneTool(addingCaption.scene, {type:"addCaption", atMs:addingCaption.atMs, text}))!==null}/>}
      {assDraft && assTarget && assDraft.scene.captionAss && <AssEditor track={assDraft.scene.captionAss} scene={assTarget.scene}
        remove={async expected=>(await onSceneTool({...assDraft.scene,revision:expected},{type:"removeAssTrack"}))!==null} revision={assDraft.scene.revision} at={assDraft.at} initialLine={assDraft.line} close={()=>setAssDraft(null)} preview={setAssPreview}
        seek={ms=>seek(assTarget.start + Math.max(0, Math.min(assTarget.durationMs-1,ms)))}
        save={(source,expected)=>onSceneTool({...assDraft.scene,revision:expected},{type:"setAssTrack",name:assDraft.scene.captionAss!.name,source})}/>}
      {captionDraft && captionTarget && <CaptionEditor key={captionTarget.scene.id}
        title={sceneLabel(captionTarget.scene, captionTarget.index)} scene={captionTarget.scene}
        draft={captionDraft} busy={busy} change={setCaptionDraft} close={() => setCaptionDraft(null)}
        seek={ms => seek(captionTarget.start + Math.min(captionTarget.durationMs - 1, Math.max(0, ms)))}
        save={(cues,expected) => onSceneTool({ ...captionTarget.scene, revision: expected }, { type: "setCaptionCues", cues })} />}
      {menu && !busy && <ClipMenu action={menu} close={() => setMenu(null)} />}
    </>
  );
}

function PreviewImage({ url, name }: { url: string; name: string }) {
  const [failed, setFailed] = useState(false);
  return failed ? <p className="preview-error">이미지를 열 수 없습니다.</p> : <img src={url} alt={name} onError={() => setFailed(true)} />;
}

function SourcePreview({ asset, mediaDir, rate, media }: { asset: Asset; mediaDir: string; rate: number; media: RefObject<HTMLMediaElement | null> }) {
  const [failed, setFailed] = useState(false);
  const url = sourceUrl(mediaDir, asset);
  useEffect(() => {
    if (media.current) {
      media.current.defaultPlaybackRate = rate;
      media.current.playbackRate = rate;
    }
  }, [rate, url]);
  return (
    <div className="picture">
      {failed ? <p className="preview-error">이 파일은 현재 미리보기에서 재생할 수 없습니다.</p> :
        asset.kind === "video" ? <video ref={el => { media.current = el; }} src={url} controls controlsList="noplaybackrate" onError={() => setFailed(true)} /> :
        asset.kind === "image" ? <PreviewImage url={url} name={asset.name} /> :
        <div className="audio-preview"><span aria-hidden="true">♪</span><strong>{asset.name}</strong><audio ref={el => { media.current = el; }} src={url} controls controlsList="noplaybackrate" onError={() => setFailed(true)} /></div>}
    </div>
  );
}

type SyncedAudioProps = {
  url: string;
  time: number;
  playing: boolean;
  rate: number;
  loop?: boolean;
  volume?: number;
  sourceRange?: BgmSourceRange | null;
};

function SyncedAudio({ url, time, playing, rate, loop = false, volume = 1, sourceRange }: SyncedAudioProps) {
  const ref = useRef<HTMLAudioElement>(null);
  const [failed, setFailed] = useState(false);
  function sync(element: HTMLAudioElement) {
    if (!element.readyState) return;
    if (element.playbackRate !== rate) element.playbackRate = rate;
    const hasDuration = Number.isFinite(element.duration);
    element.volume = volume;
    const loopStart = sourceRange ? sourceRange.inMs/1000 : 0;
    const loopEnd = sourceRange ? Math.min(element.duration, sourceRange.outMs/1000) : element.duration;
    const target = loop && hasDuration && loopEnd > loopStart ? loopStart + time % (loopEnd-loopStart) :
      hasDuration ? Math.min(time, Math.max(0, element.duration - 0.04)) : time;
    const active = playing && (loop || !hasDuration || time < element.duration - 0.04);
    if (Math.abs(element.currentTime - target) > (active ? 0.28 : 0.035) ||
        (sourceRange && (element.currentTime < loopStart || element.currentTime >= loopEnd))) element.currentTime = target;
    if (active && element.paused) void element.play().catch(() => {});
    if (!active && !element.paused) element.pause();
  }
  useEffect(() => {
    if (ref.current) sync(ref.current);
  });
  if (failed) return <p className="preview-error">이 음성 소스를 재생할 수 없습니다.</p>;
  return <audio ref={ref} src={url} loop={loop} onLoadedMetadata={event => sync(event.currentTarget)} onError={() => setFailed(true)} />;
}

function RecordingMonitor({ track }: {track: MediaStreamTrack}) {
  const ref = useRef<HTMLVideoElement>(null);
  useEffect(() => {
    const video = ref.current!;
    video.srcObject = new MediaStream([track]);
    void video.play().catch(() => {});
    return () => { video.srcObject = null; };
  }, [track]);
  return <div className="picture recording-monitor"><video ref={ref} autoPlay muted playsInline aria-label="녹화 중인 화면" /><span>● 녹화 중</span></div>;
}

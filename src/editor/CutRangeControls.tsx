import { useState } from "react";
import type { CutRangeEdit } from "./project";
import { endOf, type TrackClip } from "./visualTracks";
import { sourceTime, videoCutPoint, type VideoRange } from "./videoTrim";

type Props = {
  track: number;
  clips: TrackClip[];
  durationMs: number;
  playhead: number;
  sourceRange: (clip: TrackClip) => VideoRange | undefined;
  busy: boolean;
  pause: () => void;
  cut: (edit: CutRangeEdit, sceneStartMs: number) => void;
};

export function CutRangeControls({ track, clips, durationMs, playhead, sourceRange, busy, pause, cut }: Props) {
  const [start, setStart] = useState<number | null>(null);
  if (start === null) return <button className="button compact" disabled={busy || playhead < 0 || playhead >= durationMs}
    title={`재생바를 V${track + 1} 트랙에서 잘라낼 범위의 시작에 놓고 누르세요.`}
    onClick={() => { pause(); setStart(playhead); }}>잘라낼 시작점 지정</button>;

  const validEnd = playhead > start && playhead <= durationMs;
  const canDelete = validEnd && clips.some(c => c.startMs < playhead && endOf(c) > start);
  const clip = clips.find(c => c.startMs <= start && start < endOf(c));
  const range = clip && sourceRange(clip);
  const canSave = validEnd && clip && range &&
    videoCutPoint(range, clip.durationMs, start - clip.startMs) !== null &&
    videoCutPoint(range, clip.durationMs, playhead - clip.startMs) !== null;
  return <>
    <span className="panel-meta">V{track + 1} · 시작 {sourceTime(start)}{validEnd && ` · 끝 ${sourceTime(playhead)}`}</span>
    <button className="button compact" disabled={busy || !canDelete}
      title="이 트랙에서 시작점부터 재생바까지 한 번에 삭제합니다."
      onClick={() => cut({ type: "deleteTrackRange", track, startMs: start, endMs: playhead }, start)}>잘라내고 삭제</button>
    <button className="button compact" disabled={busy || !canSave}
      title="한 영상 클립 안에 시작점과 끝점을 지정하면 선택 범위를 잘라내 보관함에 MP4로 저장합니다."
      onClick={() => {
        if (canSave) cut({ type: "cutClipRange", disposition: "library",
          range: { clipId: clip.id, startMs: start - clip.startMs, endMs: playhead - clip.startMs } }, start);
      }}>잘라내고 보관함에 저장</button>
    <button className="text-button" disabled={busy} onClick={() => setStart(null)}>범위 취소</button>
  </>;
}

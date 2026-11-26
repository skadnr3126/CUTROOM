import { useState } from "react";
import type { CutDisposition } from "./project";
import type { TrackClip } from "./visualTracks";
import { sourceTime, videoCutPoint, videoRange } from "./videoTrim";

type Props = {
  clip: TrackClip;
  playhead: number;
  sourceMs?: number | null;
  busy: boolean;
  pause: () => void;
  cut: (startMs: number, endMs: number, disposition: CutDisposition) => void;
};

export function CutRangeControls({ clip, playhead, sourceMs, busy, pause, cut }: Props) {
  const [start, setStart] = useState<number | null>(null);
  const elapsed = playhead - clip.startMs;
  const inside = videoCutPoint(videoRange(clip, clip.durationMs, sourceMs), clip.durationMs, elapsed) !== null;
  if (start === null) return <button className="button compact" disabled={busy || !inside}
    title="재생바를 잘라낼 부분의 시작에 놓고 누르세요."
    onClick={() => { pause(); setStart(elapsed); }}>잘라낼 시작점 지정</button>;
  const validEnd = inside && elapsed > start;
  return <>
    <span className="panel-meta">시작 {sourceTime(clip.startMs + start)}{validEnd && ` · 끝 ${sourceTime(playhead)}`}</span>
    <button className="button compact" disabled={busy || !validEnd}
      title="시작점부터 현재 재생바까지 선택한 부분을 타임라인에서 삭제합니다."
      onClick={() => cut(start, elapsed, "discard")}>잘라내고 삭제</button>
    <button className="button compact" disabled={busy || !validEnd}
      title="선택한 부분을 타임라인에서 제거하고 현재 소스 보관함 폴더에 MP4로 저장합니다."
      onClick={() => cut(start, elapsed, "library")}>잘라내고 보관함에 저장</button>
    <button className="text-button" disabled={busy} onClick={() => setStart(null)}>범위 취소</button>
  </>;
}

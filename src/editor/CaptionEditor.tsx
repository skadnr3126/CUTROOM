import { useEffect, useRef, useState } from "react";
import { cueError, insertCaption, type CaptionDraft } from "./captionCues";
import type { CaptionCue, Scene } from "./project";
import { sourceTime } from "./videoTrim";

type Props = {
  title: string; scene: Scene; draft: CaptionDraft; busy: boolean;
  change: (draft: CaptionDraft) => void; seek: (ms: number) => void;
  close: () => void; save: () => Promise<boolean>;
};

export function CaptionEditor({ title, scene, draft, busy, change, seek, close, save }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  useEffect(() => { input.current?.focus(); }, [draft.index]);
  const { index, cues } = draft;
  const cue = cues[index];
  const error = cueError(cues, scene.durationMs);
  const stale = draft.revision !== scene.revision;
  function update(patch: Partial<CaptionCue>) {
    change({ ...draft, cues: cues.map((c, i) => i === index ? { ...c, ...patch } : c) });
    if (patch.startMs !== undefined && Number.isFinite(patch.startMs)) seek(patch.startMs);
    setFailed(false);
  }
  function select(list: CaptionCue[], i: number) {
    change({ ...draft, cues: list, index: i });
    if (list[i]) seek(list[i].startMs);
  }
  function next() {
    if (index + 1 < cues.length) select(cues, index + 1);
    else {
      const added = insertCaption(cues, cue?.endMs ?? 0, scene.durationMs);
      select(added.cues, added.index);
    }
  }
  async function commit() {
    if (busy || stale || error) return;
    if (await save()) close();
    else setFailed(true);
  }
  return <dialog ref={dialog} className="caption-editor" aria-labelledby="caption-editor-title"
    onCancel={e => { e.preventDefault(); if (!busy) close(); }}
    onKeyDown={e => { if (e.ctrlKey && e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); void commit(); } }}>
    <h2 id="caption-editor-title">자막 편집 <small>{title}</small></h2>
    {cue && <>
      <div className="cue-nav">
        <select aria-label="편집할 자막" value={index} onChange={e => select(cues, Number(e.target.value))}>
          {cues.map((c, i) => <option key={i} value={i}>{i + 1} · {sourceTime(c.startMs || 0)} · {c.text || "새 자막"}</option>)}
        </select>
        <button className="button compact" disabled={busy || !index} onClick={() => select(cues, index - 1)}>이전 자막</button>
      </div>
      <div className="cue-times">{(["startMs", "endMs"] as const).map(field =>
        <label key={field}>{field === "startMs" ? "시작(초)" : "끝(초)"}
          <input type="number" step="0.001" min="0" max={scene.durationMs / 1000}
            disabled={busy} key={index + field} value={Number.isFinite(cue[field]) ? cue[field] / 1000 : ""}
            onChange={e => update({ [field]: Math.round(e.target.valueAsNumber * 1000) })} /></label>)}</div>
      <textarea ref={input} aria-label="자막 내용" placeholder="화면에 표시할 문구" rows={3} disabled={busy}
        value={cue.text} onChange={e => update({ text: e.target.value })} />
      <div className="cue-nav">
        <button className="text-button" disabled={busy || !scene.text} onClick={() => update({ text: scene.text })}>대본 내용 가져오기</button>
        <button className="text-button" disabled={busy} onClick={() => {
          const list = cues.filter((_, i) => i !== index);
          select(list, Math.max(0, Math.min(index, list.length - 1)));
        }}>선택 자막 삭제</button>
      </div>
    </>}
    {!cue && <p>구간에 자막이 없습니다.</p>}
    {stale ? <p role="alert">편집 중 구간이 변경되었습니다. 닫고 다시 열어 최신 자막을 확인해 주세요.</p> :
      error ? <p role="status">{error}</p> : failed && <p role="alert">저장하지 못했습니다. 다시 시도해 주세요.</p>}
    <footer>
      <button className="button compact" disabled={busy || !!error || (index + 1 >= cues.length && cue?.endMs >= scene.durationMs)}
        onClick={next}>{index + 1 < cues.length ? "다음 자막" : "＋ 이어서 추가"}</button>
      <button className="button compact" disabled={busy} onClick={close}>취소</button>
      <button className="button compact primary" disabled={busy || stale || !!error} onClick={() => void commit()}>저장</button>
    </footer>
  </dialog>;
}

import { useEffect, useRef, useState } from "react";
import { CaptionTextField } from "./CaptionTextField";
import { sourceTime } from "./videoTrim";

export function AddCaptionDialog({ atMs, stale, save, close }: {
  atMs: number; stale: boolean; save: (text: string) => Promise<boolean>; close: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const ready = !!text.trim() && !saving && !stale;
  useEffect(() => { dialog.current?.showModal(); input.current?.focus(); }, []);
  async function submit() {
    if (!ready) return;
    setSaving(true);
    try {
      if (await save(text)) close();
      else setError("자막을 추가하지 못했습니다. 프로젝트 알림을 확인해 주세요.");
    } catch (e) { setError(String(e)); }
    finally { setSaving(false); }
  }
  return <dialog className="caption-editor" ref={dialog} aria-labelledby="add-caption-title"
    onCancel={e => { e.preventDefault(); if (!saving) close(); }}
    onKeyDown={e => { if (e.ctrlKey && e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); void submit(); } }}>
    <h2 id="add-caption-title">자막 추가 <small>구간 {sourceTime(atMs)}</small></h2>
    <CaptionTextField label="새 자막 문구" inputRef={input} value={text} disabled={saving}
      placeholder="여기에 자막을 입력하세요" onChange={setText} />
    <small>기존 자막 위에 추가하면 두 문구가 순서대로 나오도록 표시 시간을 나눕니다.</small>
    {(stale || error) && <p role="alert">{stale ? "구간이 변경되었습니다. 닫고 다시 열어 주세요." : error}</p>}
    <footer><button className="button compact" disabled={saving} onClick={close}>취소</button>
      <button className="button compact primary" disabled={!ready} onClick={() => void submit()}>추가</button></footer>
  </dialog>;
}

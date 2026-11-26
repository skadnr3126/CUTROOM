import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

// Navigation flushes any pending text using the revision acknowledged by the
// last write. Edits typed during a write are included before the flush ends.
export function useCaptionAutosave<T>(value: T, initialRevision: number, currentRevision: number,
  valid: boolean, save: (value: T, expected: number) => Promise<number | null>) {
  const saved = useRef(value);
  const revision = useRef(initialRevision);
  const pending = useRef<Promise<boolean> | null>(null);
  const [writing, setWriting] = useState(false);
  const [failure, setFailure] = useState("");
  const [composing, setComposing] = useState(false);
  const latest = useRef({ value, valid, save, composing });
  useLayoutEffect(() => { latest.current = { value, valid, save, composing }; });
  const flush = useCallback((): Promise<boolean> => {
    if (pending.current) return pending.current;
    async function write() {
      try {
        while (latest.current.value !== saved.current) {
          const draft = latest.current;
          if (!draft.valid || draft.composing) return false;
          setWriting(true);
          setFailure("");
          const next = await draft.save(draft.value, revision.current);
          if (next === null) throw new Error("저장하지 못했습니다. 다시 시도해 주세요.");
          saved.current = draft.value;
          revision.current = next;
        }
        return true;
      } catch (error) {
        setFailure(String(error));
        return false;
      }
    }
    pending.current = write().finally(() => { pending.current = null; setWriting(false); });
    return pending.current;
  }, []);
  useEffect(() => {
    if (!valid || composing || value === saved.current) return;
    const timer = window.setTimeout(() => { void flush(); }, 750);
    return () => window.clearTimeout(timer);
  }, [value, valid, composing, flush]);
  const dirty = value !== saved.current;
  const stale = !writing && currentRevision !== revision.current;
  const status = stale ? "다른 편집으로 구간이 변경되었습니다. 다시 열어 주세요."
    : failure || (writing ? "저장 중…" : dirty ? "수정 중" : "저장됨");
  return { flush, writing, dirty, stale, status, failure, revision, setComposing };
}

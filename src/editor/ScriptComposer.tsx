import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { highlightParts, type InputEdit, type TextRange } from "./scriptDraft";

type Props = {
  visible: boolean;
  busy: boolean;
  text: string;
  ranges: Map<string, TextRange>;
  activeRange?: TextRange;
  jump: TextRange | null;
  onText: (text: string, input?: InputEdit) => void;
  onReadText: () => Promise<string | null>;
  onAddSelection: (text: string, range: TextRange) => Promise<boolean>;
};

export function ScriptComposer({ visible, busy, text, ranges, activeRange, jump, onText, onReadText, onAddSelection }: Props) {
  const [hasSelection, setHasSelection] = useState(false);
  const [added, setAdded] = useState(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const highlights = useRef<HTMLDivElement>(null);
  const inputEdit = useRef<InputEdit | undefined>(undefined);
  const lastJump = useRef<TextRange | null>(null);
  const parts = useMemo(() => highlightParts(text, ranges.values(), activeRange), [text, ranges, activeRange]);

  function syncHighlights() {
    const field = textarea.current;
    const layer = highlights.current;
    if (!field || !layer) return;
    layer.style.width = `${field.clientWidth}px`;
    layer.scrollTop = field.scrollTop;
    layer.scrollLeft = field.scrollLeft;
  }

  useEffect(() => {
    const field = textarea.current;
    if (!field) return;
    const syncSelection = () => {
      const { value, selectionStart, selectionEnd } = field;
      setHasSelection(!!value.slice(selectionStart, selectionEnd).trim());
    };
    const rememberEdit = (event: InputEvent) => {
      inputEdit.current = { start: field.selectionStart, end: field.selectionEnd, type: event.inputType };
    };
    const size = new ResizeObserver(syncHighlights);
    size.observe(field);
    field.addEventListener("select", syncSelection);
    field.addEventListener("selectionchange", syncSelection);
    field.addEventListener("beforeinput", rememberEdit);
    return () => {
      size.disconnect();
      field.removeEventListener("select", syncSelection);
      field.removeEventListener("selectionchange", syncSelection);
      field.removeEventListener("beforeinput", rememberEdit);
    };
  }, []);

  useLayoutEffect(() => {
    if (visible) syncHighlights();
  }, [visible, text]);

  useLayoutEffect(() => {
    if (!visible || !jump || jump === lastJump.current) return;
    const field = textarea.current;
    const layer = highlights.current;
    if (!field || !layer) return;
    lastJump.current = jump;
    field.setSelectionRange(jump.start, jump.start);
    syncHighlights();
    const marker = layer.querySelector(`[data-script-start="${jump.start}"]`)?.getClientRects()[0];
    if (marker) {
      field.scrollTop = field.scrollTop + marker.top - layer.getBoundingClientRect().top - field.clientHeight / 4;
      syncHighlights();
    }
  }, [visible, jump]);

  function updateText(value: string, input?: InputEdit) {
    onText(value, input);
    setHasSelection(false);
    setAdded(false);
  }

  async function insertFileText() {
    const imported = await onReadText();
    if (imported !== null) {
      updateText(text.trim() ? `${text}\n\n${imported}` : imported);
    }
  }

  return (
    <form className="script-compose" hidden={!visible} onSubmit={async (event) => {
      event.preventDefault();
      const field = textarea.current;
      if (!field || busy) return;
      const { value, selectionStart, selectionEnd } = field;
      const raw = value.slice(selectionStart, selectionEnd);
      const selection = raw.trim();
      const start = selectionStart + raw.length - raw.trimStart().length;
      if (selection && await onAddSelection(selection, { start, end: start + selection.length })) {
        field.setSelectionRange(selectionEnd, selectionEnd);
        setHasSelection(false);
        setAdded(true);
      }
    }}>
      <div className="script-compose-header">
        <label htmlFor="script-draft">대본 초안</label>
        <button type="button" className="text-button" disabled={busy} onClick={() => void insertFileText()} title="TXT 또는 Markdown 파일의 텍스트를 초안 끝에 추가">파일 텍스트 가져오기</button>
      </div>
      <div className="script-input">
        <div ref={highlights} className="script-highlights" aria-hidden="true">
          {parts.map((part) => part.linked
            ? <mark className={part.active ? "current" : undefined} data-script-start={part.start} key={part.start}>{part.text}</mark>
            : <span key={part.start}>{part.text}</span>)}
          {"\u200B"}
        </div>
        <textarea id="script-draft" ref={textarea} aria-describedby="script-selection-help" value={text} disabled={busy}
          onScroll={syncHighlights}
          onChange={(event) => {
            updateText(event.target.value, inputEdit.current);
            inputEdit.current = undefined;
          }}
          placeholder="여기에 대본을 쓰세요." />
      </div>
      <p id="script-selection-help" aria-live="polite">
        {hasSelection ? "선택한 내용을 한 구간으로 추가합니다." : added ? "추가했습니다. 다음 구간을 선택할 수 있어요." : "타임라인에 넣을 부분을 대본에서 선택하세요."}
      </p>
      {ranges.size > 0 && <p className="script-legend"><i aria-hidden="true" /> 타임라인에 추가한 부분</p>}
      <button className="button primary wide" disabled={busy || !hasSelection}>선택한 구간 추가</button>
    </form>
  );
}

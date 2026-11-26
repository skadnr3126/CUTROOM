import { useRef, type RefObject } from "react";

export function CaptionTextField({ label, value, onChange, inputRef, placeholder, disabled, rows = 3 }: {
  label: string;
  value: string;
  onChange: (text: string) => void;
  inputRef?: RefObject<HTMLTextAreaElement | null>;
  placeholder?: string;
  disabled?: boolean;
  rows?: number;
}) {
  const localInput = useRef<HTMLTextAreaElement>(null);
  const input = inputRef ?? localInput;

  function insertBreak() {
    const field = input.current;
    if (!field) return;
    field.setRangeText("\n", field.selectionStart, field.selectionEnd, "end");
    onChange(field.value);
    field.focus();
  }

  return <>
    <textarea ref={input} aria-label={label} value={value} placeholder={placeholder}
      rows={rows} disabled={disabled} onChange={e => onChange(e.target.value)} />
    <button type="button" className="text-button" disabled={disabled} onClick={insertBreak}
      title="커서 위치에서 같은 자막의 다음 줄로 이동합니다.">줄바꿈 (Enter)</button>
  </>;
}

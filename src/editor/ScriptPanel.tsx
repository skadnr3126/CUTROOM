import { useLayoutEffect, useRef, useState } from "react";
import { sceneLabel, type Scene } from "./project";
import { SceneTitleInput, type SceneRename } from "./SceneTitleInput";
import { ScriptComposer } from "./ScriptComposer";
import type { useScriptDraft, TextRange } from "./scriptDraft";
import { VerticalSplit } from "./VerticalSplit";
import { useReorderDrag } from "./useReorderDrag";

export type ScriptJump = { sceneId: string; field?: "text" | "notes" | "name" };

type Props = {
  script: ReturnType<typeof useScriptDraft>;
  scenes: Scene[];
  scene: Scene | null;
  draftText: string;
  draftNotes: string;
  dirty: boolean;
  busy: boolean;
  timelineJump: ScriptJump | null;
  onChoose: (id: string) => void;
  rename: SceneRename;
  onCreate: () => void;
  onReadText: () => Promise<string | null>;
  onAddSelection: (text: string) => Promise<string | null>;
  onEdit: (field: "text" | "notes", value: string) => void;
  onSave: () => void;
  onMove: (id: string, targetId: string, after: boolean) => void;
  onRemove: (scene: Scene) => void;
};

type ScriptMode = "write" | "scenes";
const modePreference = "cutroom.script-mode.v1";

function savedMode(): ScriptMode {
  try { return localStorage.getItem(modePreference) === "scenes" ? "scenes" : "write"; }
  catch { return "write"; }
}

export function ScriptPanel({ script, scenes, scene, draftText, draftNotes, dirty, busy, timelineJump, onChoose, rename, onCreate, onReadText, onAddSelection, onEdit, onSave, onMove, onRemove }: Props) {
  const [mode, setMode] = useState(savedMode);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const reorder = useReorderDrag("application/x-cutroom-scene", "script-list", scenes.map(s => s.id), busy, onMove, "y");
  const selectedCard = useRef<HTMLButtonElement>(null);
  const selectedIndex = scenes.findIndex((entry) => entry.id === scene?.id);
  const activeRange = scene ? script.ranges.get(scene.id) : undefined;
  const [jump, setJump] = useState<TextRange | null>(null);
  const lastJump = useRef<ScriptJump | null>(null);
  useLayoutEffect(() => {
    if (!timelineJump || timelineJump === lastJump.current) return;
    lastJump.current = timelineJump;
    if (timelineJump.field) {
      chooseMode("scenes");
      setJump(null);
      return;
    }
    setJump(mode === "write" && activeRange ? { ...activeRange } : null);
  }, [timelineJump, mode, activeRange]);

  function chooseMode(next: ScriptMode) {
    setMode(next);
    try { localStorage.setItem(modePreference, next); } catch { /* Keep the preference in memory. */ }
  }

  useLayoutEffect(() => {
    setExpandedId(current => current === scene?.id ? current : null);
  }, [scene?.id]);

  useLayoutEffect(() => {
    if (mode === "scenes") selectedCard.current?.scrollIntoView({ block: "nearest" });
  }, [mode, scene?.id, timelineJump]);

  return (
    <section className="panel script-panel" id="script-pane" aria-labelledby="script-title">
      <header className="panel-header">
        <h2 id="script-title">대본</h2>
        <span className="panel-meta muted">{scenes.length}개 구간</span>
      </header>
      <div className="script-tools" role="group" aria-label="대본 보기">
        <div className="segmented script-modes">
          <button aria-pressed={mode === "write"} onClick={() => chooseMode("write")}>직접 쓰기</button>
          <button aria-pressed={mode === "scenes"} onClick={() => chooseMode("scenes")}>편집 구간</button>
        </div>
        {mode === "scenes" && <button className="button compact" disabled={busy} onClick={onCreate}>＋ 빈 구간</button>}
      </div>
      <ScriptComposer visible={mode === "write"} busy={busy} text={script.text} ranges={script.ranges}
        activeRange={activeRange} jump={jump}
        onText={script.updateText} onReadText={onReadText} onAddSelection={async (text, range) => {
          const id = await onAddSelection(text);
          if (id) script.link(id, range);
          return !!id;
        }} />
      {mode === "scenes" && (scene ? (
        <VerticalSplit className="script-split" label="기타 특이 사항 높이" controls="scene-editor"
          preference="cutroom.script-editor-height.v1" initial={46} minimum={20} maximum={75}>
          <div className="script-list">
            {scenes.map((entry, index) => {
              const selected = scene.id === entry.id;
              const expanded = selected && expandedId === entry.id;
              const label = sceneLabel(entry, index);
              const bodyId = `scene-text-${entry.id}`;
              return <div className="script-card" key={entry.id}>
                <div className="scene-card-row" onKeyDown={event => {
                  if (event.repeat || busy || (event.key !== "F2" && event.key !== "Delete")) return;
                  event.preventDefault();
                  event.stopPropagation();
                  if (event.key === "F2") { onChoose(entry.id); rename.start(entry, "script"); }
                  else onRemove(entry);
                }}>
                  {rename.active?.id === entry.id && rename.active.surface === "script"
                    ? <SceneTitleInput rename={rename} fallback={`구간 ${index + 1}`} busy={busy} saveOnBlur />
                    : <button data-scene-title className={`scene-item ${selected ? "selected" : ""}`} {...reorder(entry.id)}
                    ref={selected ? selectedCard : undefined}
                    onClick={() => onChoose(entry.id)} aria-pressed={selected}
                    onDoubleClick={() => { if (!busy) rename.start(entry, "script"); }}
                    aria-keyshortcuts="F2 Delete" title="더블클릭 또는 F2로 이름 바꾸기 · 드래그하여 구간 이동 · Delete로 삭제">
                    <strong className="scene-card-name" title={label}>{label}</strong>
                  </button>}
                  <button type="button" className="scene-disclosure" aria-expanded={expanded} aria-controls={expanded ? bodyId : undefined}
                    aria-label={`${label} 대본 ${expanded ? "접기" : "펼치기"}`} title={expanded ? "대본 접기" : "대본 펼치기"}
                    onClick={() => { onChoose(entry.id); setExpandedId(expanded ? null : entry.id); }}>
                    <span aria-hidden="true">{expanded ? "▴" : "▾"}</span>
                  </button>
                </div>
                {expanded && <div className="scene-card-body" id={bodyId}>
                  <textarea aria-label={`${label} 대본 내용`} rows={1} placeholder="대본 내용을 입력하세요."
                    disabled={busy} value={draftText} onChange={event => onEdit("text", event.target.value)} />
                  {draftText !== entry.text && <button className="button compact" disabled={busy} onClick={onSave}>저장</button>}
                </div>}
              </div>;
            })}
          </div>
          <form className="scene-editor" id="scene-editor" onSubmit={(event) => { event.preventDefault(); onSave(); }}>
            <div className="scene-editor-title">
              <button type="button" className="text-button" title="카드에서 이름 바꾸기" disabled={busy}
                onClick={() => rename.start(scene, "script")}>{sceneLabel(scene, selectedIndex)}</button>
              <div className="inline-buttons">
                <button className="button icon danger" type="button" disabled={busy} onClick={() => onRemove(scene)} aria-label="구간 삭제">×</button>
                <button className="button primary compact" disabled={busy || !dirty}>저장</button>
              </div>
            </div>
            <label htmlFor="scene-notes">기타 특이 사항</label>
            <textarea key={scene.id} id="scene-notes" disabled={busy}
              placeholder="이 구간의 편집 메모를 적으세요."
              value={draftNotes} onChange={event => onEdit("notes", event.target.value)} />
          </form>
        </VerticalSplit>
      ) : (
        <div className="center-empty"><span className="source-glyph" aria-hidden="true">≡</span><strong>타임라인에 추가한 대본이 여기에 표시됩니다.</strong><button className="button compact" onClick={() => chooseMode("write")}>대본 쓰기</button></div>
      ))}
    </section>
  );
}

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke, type ProjectSession } from "./projectSession";
import type { Scene } from "./project";

export type TextRange = { start: number; end: number };
export type InputEdit = TextRange & { type: string };
type Draft = { text: string; locations: Record<string, TextRange | null> };

const storageKey = "cutroom.script-draft.v2";

function normalize(text: string) {
  return text.replace(/\r\n?/g, "\n");
}

function isDraft(value: unknown): value is Draft {
  if (!value || typeof value !== "object") return false;
  return "text" in value && typeof value.text === "string" && "locations" in value &&
    !!value.locations && typeof value.locations === "object";
}

function cachedDraft(key: string): Draft | null {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? "null");
    return isDraft(value) ? value : null;
  } catch { return null; }
}

function readDraft(session: ProjectSession): Draft {
  const recovery = cachedDraft(storageKey + "." + session.id);
  if (recovery) return recovery;
  if (isDraft(session.draft)) return session.draft;
  if (session.legacy) {
    const previous = cachedDraft(storageKey);
    if (previous) return previous;
    try { return { text: normalize(localStorage.getItem("cutroom.script-draft") ?? ""), locations: {} }; }
    catch { /* The project folder is still available. */ }
  }
  return {text: "", locations: {}};
}

function hasLocation(draft: Draft, id: string) {
  return Object.prototype.hasOwnProperty.call(draft.locations, id);
}

function resolve(draft: Draft, scene: Scene): TextRange | null {
  if (hasLocation(draft, scene.id)) {
    const range = draft.locations[scene.id];
    return range && Number.isInteger(range.start) && Number.isInteger(range.end) &&
      range.start >= 0 && range.start < range.end && range.end <= draft.text.length ? range : null;
  }
  // Older scenes have text as their only evidence of a source location.
  const excerpt = normalize(scene.text).trim();
  const start = excerpt ? draft.text.indexOf(excerpt) : -1;
  if (start < 0 || draft.text.indexOf(excerpt, start + 1) >= 0) return null;
  return { start, end: start + excerpt.length };
}

function changedRange(before: string, after: string, input?: InputEdit) {
  const delta = after.length - before.length;
  if (input) {
    let { start, end } = input;
    if (start === end && delta < 0) {
      if (input.type.endsWith("Backward")) start += delta;
      if (input.type.endsWith("Forward")) end -= delta;
    }
    const newEnd = end + delta;
    if (start >= 0 && end <= before.length && newEnd >= start &&
      before.slice(0, start) === after.slice(0, start) &&
      before.slice(end) === after.slice(newEnd)) {
      return { start, end, newEnd };
    }
  }
  let start = 0;
  const limit = Math.min(before.length, after.length);
  while (start < limit && before[start] === after[start]) start++;
  let end = before.length;
  let newEnd = after.length;
  while (end > start && newEnd > start && before[end - 1] === after[newEnd - 1]) {
    end--;
    newEnd--;
  }
  return { start, end, newEnd };
}

function moveRange(range: TextRange, edit: ReturnType<typeof changedRange>): TextRange | null {
  const delta = edit.newEnd - edit.end;
  if (range.end <= edit.start) return range;
  if (range.start >= edit.end) return { start: range.start + delta, end: range.end + delta };
  if (edit.start <= range.start && range.end <= edit.end &&
    (edit.start !== range.start || edit.end !== range.end || edit.start === edit.newEnd)) return null;
  return {
    start: Math.min(range.start, edit.start),
    end: range.end > edit.end ? range.end + delta : edit.newEnd,
  };
}

export function useScriptDraft(scenes: Scene[], session:ProjectSession) {
  const [draft, setDraft] = useState(() => readDraft(session));
  const queue = useRef(Promise.resolve());
  const lastSaved = useRef(JSON.stringify(session.draft ?? {text:"",locations:{}}));
  const recoveryKey = storageKey + "." + session.id;
  function flush() {
    const value = JSON.stringify(draft);
    queue.current = queue.current.catch(() => {}).then(async () => {
      if (lastSaved.current !== value) {
        await invoke("project_save_draft", { draft });
        lastSaved.current = value;
      }
      try {
        if (localStorage.getItem(recoveryKey) === value) localStorage.removeItem(recoveryKey);
      } catch { /* The project folder already has the draft. */ }
    });
    return queue.current;
  }
  const ranges = useMemo(() => {
    const matches = new Map<string, TextRange>();
    for (const scene of scenes) {
      const range = resolve(draft, scene);
      if (range) matches.set(scene.id, range);
    }
    return matches;
  }, [draft, scenes]);

  useLayoutEffect(() => {
    const value = JSON.stringify(draft);
    try {
      if (lastSaved.current === value && !localStorage.getItem(recoveryKey)) return;
      localStorage.setItem(recoveryKey, value);
    } catch { /* Save to the project folder when browser storage is unavailable. */ }
    const timer = window.setTimeout(() => { void flush().catch(() => {}); }, 240);
    return () => window.clearTimeout(timer);
  }, [draft, recoveryKey]);

  useEffect(() => {
    setDraft((current) => {
      const recovered: Draft["locations"] = {};
      for (const scene of scenes) {
        if (hasLocation(current, scene.id)) continue;
        const range = resolve(current, scene);
        if (range) recovered[scene.id] = range;
      }
      return Object.keys(recovered).length
        ? { ...current, locations: { ...current.locations, ...recovered } }
        : current;
    });
  }, [scenes]);

  function updateText(value: string, input?: InputEdit) {
    const text = normalize(value);
    setDraft((current) => {
      if (text === current.text) return current;
      const edit = changedRange(current.text, text, input);
      const locations = Object.fromEntries(scenes.map((scene) => {
        const range = resolve(current, scene);
        const moved = range && moveRange(range, edit);
        return [scene.id, moved && text.slice(moved.start, moved.end).trim() ? moved : null];
      }));
      return { text, locations };
    });
  }

  function link(id: string, range: TextRange) {
    setDraft((current) => ({ ...current, locations: { ...current.locations, [id]: range } }));
  }

  return { text: draft.text, ranges, updateText, link, flush };
}

export function highlightParts(text: string, ranges: Iterable<TextRange>, active?: TextRange) {
  const changes = new Map([[0, 0], [text.length, 0]]);
  for (const { start, end } of ranges) {
    changes.set(start, (changes.get(start) ?? 0) + 1);
    changes.set(end, (changes.get(end) ?? 0) - 1);
  }
  const boundaries = [...changes.keys()].sort((a, b) => a - b);
  let depth = 0;
  return boundaries.slice(0, -1).map((start, i) => {
    depth += changes.get(start) ?? 0;
    return {
      start,
      text: text.slice(start, boundaries[i + 1]),
      linked: depth > 0,
      active: active && start >= active.start && start < active.end,
    };
  });
}

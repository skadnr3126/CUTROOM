import { useEffect, useRef, useState } from "react";
import { invoke } from "./projectSession";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
import { captureError, startScreenRecording } from "./screenRecording";
import { useRecordingRegionPicker } from "./RecordingRegionPicker";
import { recordingFormat, type RecordingAudioMode } from "./recordingFormat";

type Props = {
  onActivity: (active:boolean) => void;
  busy: boolean;
  ffmpegAvailable: boolean;
  onCapture: () => void;
  onPreview: (track: MediaStreamTrack | null) => void;
  onSave: (id: string) => Promise<boolean>;
};
type Phase = "idle" | "choosing" | "recording" | "saving" | "failed";
const shortcut = "Ctrl+Shift+F9";

export function CaptureControls({ onActivity, busy, ffmpegAvailable, onCapture, onPreview, onSave }: Props) {
  const [options, setOptions] = useState(false);
  const [regionMode, setRegionMode] = useState(false);
  const region = useRecordingRegionPicker();
  const [systemAudio, setSystemAudio] = useState(true);
  const [microphone, setMicrophone] = useState(false);
  const [audioMode, setAudioMode] = useState<RecordingAudioMode>(() => recordingFormat("pcm") ? "pcm" : "compressed");
  const [phase, setPhase] = useState<Phase>("idle");
  useEffect(() => { onActivity(phase !== "idle" && (phase !== "failed" || !!unsaved.current)); }, [phase, onActivity]);
  const [hint, setHint] = useState("");
  const [seconds, setSeconds] = useState(0);
  const [hotkey, setHotkey] = useState(false);
  const stopCapture = useRef<(() => void) | null>(null);
  const unsaved = useRef<string | null>(null);
  const started = useRef(0);
  const session = useRef<AbortController | null>(null);

  useEffect(() => {
    const release = () => session.current?.abort();
    window.addEventListener("pagehide", release);
    return () => { window.removeEventListener("pagehide", release); release(); };
  }, []);

  function stop() {
    if (!stopCapture.current) return;
    onPreview(null);
    stopCapture.current();
    stopCapture.current = null;
    setPhase("saving");
    setHint("");
  }

  useEffect(() => {
    if (phase !== "recording") return;
    const timer = window.setInterval(() => setSeconds(Math.floor((Date.now() - started.current) / 1000)), 250);
    let active = true;
    const registered = register(shortcut, event => { if (active && event.state === "Pressed") stop(); })
      .then(() => { if (active) setHotkey(true); return true; }).catch(() => false);
    return () => {
      active = false;
      clearInterval(timer);
      void registered.then(ok => { if (ok) return unregister(shortcut); }).catch(() => {});
    };
  }, [phase]);

  useEffect(() => {
    if (phase === "idle" || phase === "failed") return;
    const listener = getCurrentWindow().onCloseRequested(event => {
      event.preventDefault();
      if (stopCapture.current) stop();
      else if (phase === "choosing") session.current?.abort();
    });
    return () => { void listener.then(remove => remove()).catch(() => {}); };
  }, [phase]);

  async function save(id: string) {
    unsaved.current = id;
    setPhase("saving");
    setHint("");
    if (await onSave(id)) {
      unsaved.current = null;
      setPhase("idle");
    } else setPhase("failed");
  }

  async function start() {
    setOptions(false);
    setHint("");
    setHotkey(false);
    setPhase("choosing");
    const controller = new AbortController();
    session.current = controller;
    try {
      const capture = await startScreenRecording({ systemAudio, microphone, audioMode,
        selectRegion: regionMode ? region.choose : undefined }, controller.signal, result => {
        onPreview(null);
        session.current = null;
        stopCapture.current = null;
        if ("id" in result) void save(result.id).then(() => { if (result.notice) setHint(result.notice); });
        else { setHint(result.error); setPhase("idle"); }
      });
      onPreview(capture.previewTrack);
      stopCapture.current = capture.stop;
      started.current = Date.now();
      setSeconds(0);
      setPhase("recording");
    } catch (error) { session.current = null; setHint(captureError(error)); setPhase("idle"); setOptions(true); }
  }

  return <div className="capture-controls">
    <div className="capture-actions">
      <button className="button compact" disabled={busy} onClick={onCapture}>영역 캡처</button>
      {phase === "idle" && <button className="button compact" disabled={busy || !ffmpegAvailable}
        title={ffmpegAvailable ? "화면 녹화 설정" : "녹화하려면 FFmpeg가 필요합니다."}
        aria-expanded={options} onClick={() => setOptions(!options)}>화면 녹화</button>}
      {phase === "recording" && <button className="button compact recording-stop" onClick={stop}
        title={hotkey ? `녹화 중지 (${shortcut})` : "녹화 중지"}>
        ■ 중지 {Math.floor(seconds / 60)}:{String(seconds % 60).padStart(2, "0")}
      </button>}
      {phase === "recording" && hotkey && <small>{shortcut}</small>}
      {phase === "choosing" && <small role="status">녹화 대상 선택 중…</small>}
      {phase === "saving" && <small role="status">MP4 저장 중…</small>}
      {phase === "failed" && <>
        <button className="button compact" disabled={busy} onClick={() => { if (unsaved.current) void save(unsaved.current); }}>저장 다시 시도</button>
        <button className="button compact" onClick={async () => {
          if (!window.confirm("저장하지 못한 녹화본을 삭제할까요?")) return;
          try {
            await invoke("discard_recording", { id: unsaved.current });
            unsaved.current = null; setPhase("idle"); setHint("");
          } catch (error) { setHint(captureError(error)); }
        }}>녹화본 삭제</button>
      </>}
    </div>
    {options && phase === "idle" && <div className="capture-actions">
      <label>녹화 범위 <select aria-label="녹화 범위" value={regionMode ? "region" : "full"}
        onChange={e => setRegionMode(e.target.value === "region")}>
        <option value="full">전체 화면 또는 창</option><option value="region">영역 지정</option>
      </select></label>
      <label><input type="checkbox" checked={systemAudio} onChange={e => setSystemAudio(e.target.checked)} />컴퓨터 소리</label>
      <label><input type="checkbox" checked={microphone} onChange={e => setMicrophone(e.target.checked)} />마이크</label>
      {(systemAudio || microphone) && <label>오디오 방식 <select value={audioMode}
        onChange={e => setAudioMode(e.target.value === "pcm" ? "pcm" : "compressed")}>
        <option value="pcm" disabled={!recordingFormat("pcm")}>원음 캡처 (PCM)</option>
        <option value="compressed" disabled={!recordingFormat("compressed")}>압축 캡처</option>
      </select></label>}
      <button className="button compact primary" disabled={busy} onClick={() => void start()}>녹화 시작</button>
    </div>}
    {hint && <small role="status">{hint}</small>}
    {region.picker}
  </div>;
}

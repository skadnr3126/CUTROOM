import { invoke } from "./projectSession";
import { prepareRegionTrack, type SelectRegion } from "./recordingRegion";
import { recordingFormat, type RecordingAudioMode } from "./recordingFormat";

type Options = { systemAudio: boolean; microphone: boolean; audioMode: RecordingAudioMode; selectRegion?: SelectRegion };
type Result = { id: string; notice?: string } | { error: string };

export function captureError(error: unknown) {
  if (error instanceof DOMException && error.name === "NotAllowedError")
    return "녹화 대상 선택이 취소되었거나 화면 또는 마이크 사용 권한이 없습니다.";
  return error instanceof Error ? error.message : String(error);
}

// MediaRecorder chunks belong to one continuous stream. Write in event order
// and wait for the final dataavailable event before finalizing the MP4.
export async function startScreenRecording(options: Options, signal: AbortSignal, finished: (result: Result) => void) {
  if (!navigator.mediaDevices?.getDisplayMedia || typeof MediaRecorder === "undefined")
    throw new Error("이 환경에서 화면 녹화를 사용할 수 없습니다. WebView2를 업데이트해 주세요.");
  const withAudio = options.systemAudio || options.microphone;
  const format = recordingFormat(withAudio ? options.audioMode : "silent");
  if (!format) throw new Error("선택한 녹화 방식에 사용할 코덱을 찾을 수 없습니다. 녹화 설정을 확인해 주세요.");
  const display = await navigator.mediaDevices.getDisplayMedia({
    video: { frameRate: { ideal: 30, max: 30 } },
    audio: options.systemAudio && { echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 2 },
    systemAudio: options.systemAudio ? "include" : "exclude",
    windowAudio: options.systemAudio ? "window" : "exclude",
  } as DisplayMediaStreamOptions);
  const tracks = new Set(display.getTracks());
  let crop: Awaited<ReturnType<typeof prepareRegionTrack>> | undefined;
  let mixer: AudioContext | undefined;
  let id: string | undefined;
  const release = () => {
    crop?.close();
    for (const track of tracks) track.stop();
    if (mixer && mixer.state !== "closed") void mixer.close().catch(() => {});
  };
  // Release a selected screen even if a separate microphone prompt is still open.
  signal.addEventListener("abort", release, { once: true });
  try {
    signal.throwIfAborted();
    const video = display.getVideoTracks()[0];
    const audio = options.systemAudio ? display.getAudioTracks().filter(t => t.readyState === "live") : [];
    if (options.systemAudio && !audio.length)
      throw new Error("컴퓨터 소리가 공유되지 않았습니다. 전체 화면을 선택하고 공유 창에서 오디오 공유를 켜 주세요. 컴퓨터 소리가 필요 없으면 녹화 설정에서 ‘컴퓨터 소리’를 꺼 주세요.");
    if (options.selectRegion) crop = await prepareRegionTrack(video, signal, options.selectRegion);
    signal.throwIfAborted();
    if (options.microphone) {
      const mic = await navigator.mediaDevices.getUserMedia({ audio: options.audioMode === "pcm"
        ? { noiseSuppression: false, autoGainControl: false, echoCancellation: options.systemAudio }
        : true });
      for (const track of mic.getTracks()) tracks.add(track);
      const micAudio = mic.getAudioTracks().filter(t => t.readyState === "live");
      if (!micAudio.length) throw new Error("마이크 소리를 받을 수 없습니다. 마이크 연결과 권한을 확인해 주세요.");
      audio.push(...micAudio);
      signal.throwIfAborted();
    }
    const stream = new MediaStream([crop?.track ?? video]);
    if (audio.length > 1) {
      mixer = new AudioContext();
      const mix = mixer.createMediaStreamDestination();
      const gain = mixer.createGain();
      gain.gain.value = 1 / audio.length;
      gain.connect(mix);
      for (const track of audio) mixer.createMediaStreamSource(new MediaStream([track])).connect(gain);
      await mixer.resume();
      const track = mix.stream.getAudioTracks()[0];
      stream.addTrack(track);
      tracks.add(track);
    } else if (audio.length) stream.addTrack(audio[0]);
    const encoderOptions: MediaRecorderOptions = { mimeType: format.mimeType };
    if (withAudio && options.audioMode === "compressed") encoderOptions.audioBitsPerSecond = 192_000;
    const recorder = new MediaRecorder(stream, encoderOptions);
    id = await invoke<string>("begin_recording", { container: format.container, audio: format.audio });
    const recordingId = id;
    signal.throwIfAborted();
    let writes = Promise.resolve();
    let failure: string | undefined;
    let notice: string | undefined;
    let offset = 0;
    let stopped = false;
    const stop = () => {
      if (stopped) return;
      stopped = true;
      signal.removeEventListener("abort", stop);
      if (recorder.state !== "inactive") recorder.stop();
      release();
    };
    recorder.ondataavailable = ({ data }) => {
      if (!data.size) return;
      writes = writes.then(async () => {
        if (failure) return;
        for (let pos = 0; pos < data.size; pos += 2 ** 20) {
          const bytes = await data.slice(pos, pos + 2 ** 20).arrayBuffer();
          await invoke("append_recording", bytes, { headers: {
            "x-recording-id": recordingId, "x-recording-offset": String(offset),
          } });
          offset += bytes.byteLength;
        }
      }).catch(error => { failure = captureError(error); stop(); });
    };
    recorder.onerror = () => { failure = "화면 녹화 중 오류가 발생했습니다."; stop(); };
    recorder.onstop = async () => {
      stop();
      await writes;
      if (failure || !offset) {
        await invoke("discard_recording", { id: recordingId }).catch(() => {});
        finished({ error: failure ?? notice ?? "녹화된 화면이 없습니다." });
      } else finished({ id: recordingId, notice });
    };
    if (video.readyState === "ended") throw new Error("화면 공유가 종료되었습니다.");
    if (audio.some(track => track.readyState === "ended")) throw new Error("녹화 시작 전에 오디오 연결이 종료되었습니다.");
    signal.removeEventListener("abort", release);
    signal.addEventListener("abort", stop, { once: true });
    video.addEventListener("ended", stop, { once: true });
    for (const track of audio) track.addEventListener("ended", () => {
      if (stopped) return;
      notice = "오디오 연결이 끊겨 그 시점까지 녹화했습니다.";
      stop();
    }, { once: true });
    recorder.start(1000);
    if (crop) void crop.pump().then(stop, () => {
      if (stopped) return;
      notice = "영역 영상 처리가 중단되어 녹화를 종료했습니다.";
      stop();
    });
    return { stop, previewTrack: crop?.track ?? video };
  } catch (error) {
    signal.removeEventListener("abort", release);
    release();
    if (id) await invoke("discard_recording", { id }).catch(() => {});
    throw error;
  }
}

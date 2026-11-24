export type RecordingAudioMode = "pcm" | "compressed";

// These containers are paired with video encoders supported by the MP4 finalizer.
const formats = [
  { mode: "pcm", mimeType: "video/x-matroska;codecs=avc1,pcm", container: "matroska", audio: "pcm" },
  { mode: "pcm", mimeType: "video/webm;codecs=vp8,pcm", container: "webm", audio: "pcm" },
  { mode: "compressed", mimeType: "video/mp4;codecs=avc1,mp4a.40.2", container: "mp4", audio: "aac" },
  { mode: "compressed", mimeType: "video/mp4;codecs=avc1,opus", container: "mp4", audio: "opus" },
  { mode: "compressed", mimeType: "video/webm;codecs=vp8,opus", container: "webm", audio: "opus" },
  { mode: "silent", mimeType: "video/mp4;codecs=avc1", container: "mp4", audio: "none" },
  { mode: "silent", mimeType: "video/webm;codecs=vp8", container: "webm", audio: "none" },
] as const;

export function recordingFormat(mode: RecordingAudioMode | "silent") {
  return formats.find(format => format.mode === mode && globalThis.MediaRecorder?.isTypeSupported(format.mimeType));
}

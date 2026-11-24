use super::super::{ProjectStore, RenderedSource, files, run_io};
use super::{register, staged_file};
use serde::Deserialize;
use std::{collections::HashMap, io::Write, path::Path, sync::Mutex};
use tauri::{
    State,
    ipc::{InvokeBody, Request},
};
use tempfile::NamedTempFile;
use uuid::Uuid;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Container {
    Mp4,
    Matroska,
    Webm,
}

#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Audio {
    None,
    Aac,
    Opus,
    Pcm,
}

struct Recording {
    input: NamedTempFile,
    container: Container,
    audio: Audio,
    offset: u64,
}

impl Recording {
    fn append(&mut self, offset: u64, data: &[u8]) -> Result<(), String> {
        if offset != self.offset {
            return Err("녹화 데이터의 순서가 맞지 않습니다.".into());
        }
        self.input
            .write_all(data)
            .map_err(|_| "녹화 데이터를 쓸 수 없습니다. 디스크 공간을 확인해 주세요.")?;
        self.offset += data.len() as u64;
        Ok(())
    }

    fn finish(
        &self,
        store: &ProjectStore,
        ffmpeg: &Path,
        folder_id: Option<Uuid>,
    ) -> Result<RenderedSource, String> {
        let mp4 = staged_file(store, ".mp4")?;
        let mut cmd = files::media_command(ffmpeg);
        cmd.args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(self.input.path())
            .args(["-map", "0:v:0"]);
        match self.audio {
            Audio::None => {
                cmd.arg("-an");
            }
            Audio::Aac => {
                cmd.args(["-map", "0:a:0", "-c:a", "copy"]);
            }
            Audio::Pcm | Audio::Opus => {
                cmd.args(["-map", "0:a:0", "-c:a", "aac", "-b:a", "192k"]);
            }
        }
        if matches!(self.container, Container::Mp4 | Container::Matroska) {
            cmd.args(["-c:v", "copy"]);
        } else {
            cmd.args([
                "-vf",
                "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv420p",
            ]);
        }
        let output = cmd
            .args(["-movflags", "+faststart"])
            .arg(mp4.path())
            .output()
            .map_err(|_| "녹화본을 저장할 FFmpeg를 실행할 수 없습니다.")?;
        if !output.status.success() {
            if self.audio != Audio::None
                && String::from_utf8_lossy(&output.stderr).contains("0:a:0")
                && String::from_utf8_lossy(&output.stderr).contains("matches no streams")
            {
                return Err("녹화 데이터에 요청한 오디오 트랙이 없습니다. 공유 창의 오디오 설정을 확인하고 다시 녹화해 주세요.".into());
            }
            return Err(
                "녹화본을 MP4로 저장할 수 없습니다. 디스크 공간을 확인하고 다시 시도해 주세요."
                    .into(),
            );
        }
        register(
            store,
            mp4,
            "화면 녹화.mp4",
            folder_id,
            self.audio != Audio::None,
        )
    }
}

#[derive(Default)]
pub(crate) struct RecordingInbox(Mutex<HashMap<Uuid, Recording>>);

#[tauri::command]
pub(crate) fn begin_recording(
    store: ProjectStore,
    inbox: State<'_, RecordingInbox>,
    container: Container,
    audio: Audio,
) -> Result<Uuid, String> {
    files::find_tool(&store.app_data, "ffmpeg")
        .ok_or("녹화에 필요한 FFmpeg를 찾을 수 없습니다.")?;
    let id = Uuid::new_v4();
    let input = staged_file(
        &store,
        match container {
            Container::Mp4 => ".mp4",
            Container::Matroska => ".mkv",
            Container::Webm => ".webm",
        },
    )?;
    let mut slot = inbox
        .0
        .lock()
        .map_err(|_| "녹화 저장 공간을 열 수 없습니다.")?;
    slot.insert(
        id,
        Recording {
            input,
            container,
            audio,
            offset: 0,
        },
    );
    Ok(id)
}

#[tauri::command]
pub(crate) fn append_recording(
    inbox: State<'_, RecordingInbox>,
    request: Request<'_>,
) -> Result<(), String> {
    let header = |key: &str| request.headers().get(key).and_then(|v| v.to_str().ok());
    let id = header("x-recording-id")
        .and_then(|s| s.parse::<Uuid>().ok())
        .ok_or("녹화 번호를 읽을 수 없습니다.")?;
    let offset = header("x-recording-offset")
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or("녹화 위치를 읽을 수 없습니다.")?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("녹화 데이터를 읽을 수 없습니다.".into());
    };
    let mut slot = inbox.0.lock().map_err(|_| "녹화 파일을 열 수 없습니다.")?;
    slot.get_mut(&id)
        .ok_or("종료된 녹화입니다.")?
        .append(offset, bytes)
}

#[tauri::command]
pub(crate) async fn finish_recording(
    store: ProjectStore,
    inbox: State<'_, RecordingInbox>,
    id: Uuid,
    folder_id: Option<Uuid>,
) -> Result<RenderedSource, String> {
    let ffmpeg = files::find_tool(&store.app_data, "ffmpeg").ok_or("FFmpeg를 찾을 수 없습니다.")?;
    let recording = inbox
        .0
        .lock()
        .map_err(|_| "녹화 파일을 열 수 없습니다.")?
        .remove(&id)
        .ok_or("저장할 녹화를 찾을 수 없습니다.")?;
    let (recording, result) = run_io(move || {
        let result = if recording.input.path().parent() == Some(store.media_dir().as_path()) {
            recording.finish(&store, &ffmpeg, folder_id)
        } else {
            Err("다른 프로젝트의 녹화입니다.".into())
        };
        Ok((recording, result))
    })
    .await?;
    if result.is_err() {
        inbox
            .0
            .lock()
            .map_err(|_| "녹화 파일을 열 수 없습니다.")?
            .insert(id, recording);
    }
    result
}

#[tauri::command]
pub(crate) fn discard_recording(inbox: State<'_, RecordingInbox>, id: Uuid) -> Result<(), String> {
    inbox
        .0
        .lock()
        .map_err(|_| "녹화 파일을 열 수 없습니다.")?
        .remove(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Run with CUTROOM_TEST_TOOLS pointing to FFmpeg"]
    fn recording_chunks_finalize_as_seekable_mp4_sources() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(root.path().into());
        let tools = std::path::PathBuf::from(std::env::var_os("CUTROOM_TEST_TOOLS").unwrap());
        let exe = tools.join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX));
        let probe = tools.join(format!("ffprobe{}", std::env::consts::EXE_SUFFIX));
        let cases = [
            (Container::Mp4, Audio::None),
            (Container::Mp4, Audio::Aac),
            (Container::Webm, Audio::Opus),
            (Container::Matroska, Audio::Pcm),
            (Container::Webm, Audio::Pcm),
        ];
        for (container, audio) in cases {
            let with_audio = audio != Audio::None;
            let mut cmd = files::media_command(&exe);
            cmd.args(["-v", "error", "-f", "lavfi", "-i", "color=s=96x64:r=10"]);
            if with_audio {
                cmd.args([
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440",
                    "-c:a",
                    match audio {
                        Audio::Aac => "aac",
                        Audio::Pcm => "pcm_s16le",
                        Audio::Opus => "libopus",
                        Audio::None => unreachable!(),
                    },
                ]);
            }
            match container {
                Container::Mp4 => cmd.args([
                    "-c:v",
                    "libx264",
                    "-bf",
                    "0",
                    "-f",
                    "mp4",
                    "-movflags",
                    "frag_keyframe+empty_moov",
                ]),
                Container::Matroska => cmd.args(["-c:v", "libx264", "-bf", "0", "-f", "matroska"]),
                Container::Webm => cmd.args([
                    "-c:v",
                    "libvpx",
                    "-f",
                    if audio == Audio::Pcm {
                        "matroska"
                    } else {
                        "webm"
                    },
                ]),
            };
            let encoded = cmd.args(["-t", "1", "pipe:1"]).output().unwrap();
            assert!(encoded.status.success());
            let mut recording = Recording {
                container,
                audio,
                input: staged_file(&store, ".tmp").unwrap(),
                offset: 0,
            };
            assert!(recording.append(1, &[0]).is_err());
            for chunk in encoded.stdout.chunks(128) {
                recording.append(recording.offset, chunk).unwrap();
            }
            if !with_audio {
                recording.audio = Audio::Aac;
                assert!(recording.finish(&store, &exe, None).is_err());
                recording.audio = Audio::None;
            }
            let saved = recording.finish(&store, &exe, None).unwrap();
            let asset = store
                .load()
                .unwrap()
                .assets
                .into_iter()
                .find(|a| a.id == saved.asset_id)
                .unwrap();
            assert_eq!(asset.extension, "mp4");
            let output = asset.path_in(&store.media_dir());
            let inspected = files::inspect_asset(&output, &asset.name, Some(&probe)).unwrap();
            assert_eq!(inspected.has_audio, with_audio);
            assert_eq!(asset.has_audio, with_audio);
            if with_audio {
                let pcm = files::media_command(&exe)
                    .args(["-v", "error", "-i"])
                    .arg(&output)
                    .args(["-map", "0:a:0", "-f", "s16le", "-"])
                    .output()
                    .unwrap();
                assert!(
                    pcm.status.success()
                        && pcm
                            .stdout
                            .chunks_exact(2)
                            .any(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs() > 200)
                );
            }
            assert!((950..1200).contains(&inspected.duration_ms.unwrap()));
        }
        assert_eq!(store.load().unwrap().assets.len(), cases.len());
    }
}

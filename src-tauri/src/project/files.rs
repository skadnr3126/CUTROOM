use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use tempfile::NamedTempFile;
use uuid::Uuid;

use super::model::{Asset, Kind};

pub(super) const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "webm", "m4v", "png", "jpg", "jpeg", "webp", "wav", "mp3", "m4a", "aac", "ogg",
    "flac",
];

pub(super) fn read_script(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|_| "대본 파일을 읽을 수 없습니다.")?;
    if data.starts_with(&[0xff, 0xfe]) || data.starts_with(&[0xfe, 0xff]) {
        let little_endian = data.starts_with(&[0xff, 0xfe]);
        let (units, remainder) = data[2..].as_chunks::<2>();
        if !remainder.is_empty() {
            return Err("대본 파일의 UTF-16 형식이 올바르지 않습니다.".to_owned());
        }
        let units: Vec<u16> = units
            .iter()
            .map(|unit| {
                if little_endian {
                    u16::from_le_bytes(*unit)
                } else {
                    u16::from_be_bytes(*unit)
                }
            })
            .collect();
        return String::from_utf16(&units)
            .map_err(|_| "대본 파일의 UTF-16 형식이 올바르지 않습니다.".to_owned());
    }
    let data = data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&data);
    std::str::from_utf8(data)
        .map(str::to_owned)
        .map_err(|_| "대본을 UTF-8 또는 UTF-16 형식으로 저장한 뒤 다시 열어 주세요.".to_owned())
}

pub(super) fn copy_asset(path: &Path, media: &Path, probe: Option<&Path>) -> Result<Asset, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("소스 파일 이름을 읽을 수 없습니다.")?;
    let asset = inspect_asset(path, name, probe)?;
    let staged = stage_copy(path, media, name)?;
    persist_asset(staged, media, &asset)?;
    Ok(asset)
}

pub(super) fn stage_copy(path: &Path, media: &Path, name: &str) -> Result<NamedTempFile, String> {
    let mut staged = NamedTempFile::new_in(media).map_err(|_| "소스 복사를 시작할 수 없습니다.")?;
    let mut input =
        fs::File::open(path).map_err(|_| format!("소스 파일을 열 수 없습니다. ({name})"))?;
    if !input.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(format!("일반 파일을 선택해 주세요. ({name})"));
    }
    io::copy(&mut input, &mut staged)
        .map_err(|_| format!("소스를 프로젝트에 복사할 수 없습니다. ({name})"))?;
    Ok(staged)
}

pub(super) fn inspect_asset(
    path: &Path,
    name: &str,
    probe: Option<&Path>,
) -> Result<Asset, String> {
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind = Kind::from_extension(&extension)
        .ok_or_else(|| format!("지원하는 소스 형식이 아닙니다. ({name})"))?;
    let (duration_ms, has_audio) = inspect(path, kind, probe)
        .map_err(|_| format!("소스 파일을 읽을 수 없습니다. ({name})"))?;
    Ok(Asset {
        disk: None,
        missing: false,
        project_file: None,
        folder_id: None,
        id: Uuid::new_v4(),
        name: name.to_owned(),
        extension,
        kind,
        duration_ms,
        has_audio,
        source_job_id: None,
        removed_from_bin: false,
    })
}

pub(super) fn persist_asset(
    staged: NamedTempFile,
    media: &Path,
    asset: &Asset,
) -> Result<(), String> {
    staged
        .as_file()
        .sync_all()
        .map_err(|_| format!("소스를 저장할 수 없습니다. ({})", asset.name))?;
    staged
        .persist_noclobber(asset.path_in(media))
        .map_err(|_| format!("소스를 프로젝트에 저장할 수 없습니다. ({})", asset.name))?;
    Ok(())
}

pub(super) fn write_title(media: &Path, encoded: &str) -> Result<Asset, String> {
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "타이틀 카드 이미지를 읽을 수 없습니다.")?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("타이틀 카드는 PNG 형식이어야 합니다.".to_owned());
    }
    let asset = Asset {
        disk: None,
        missing: false,
        project_file: None,
        folder_id: None,
        id: Uuid::new_v4(),
        name: "대본 타이틀 카드.png".to_owned(),
        extension: "png".to_owned(),
        kind: Kind::Image,
        duration_ms: None,
        has_audio: false,
        source_job_id: None,
        removed_from_bin: false,
    };
    let mut staged =
        NamedTempFile::new_in(media).map_err(|_| "타이틀 카드를 저장할 수 없습니다.")?;
    staged
        .write_all(&bytes)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|_| "타이틀 카드를 저장할 수 없습니다.")?;
    staged
        .persist_noclobber(asset.path_in(media))
        .map_err(|_| "타이틀 카드를 저장할 수 없습니다.")?;
    Ok(asset)
}

fn inspect(path: &Path, kind: Kind, probe: Option<&Path>) -> Result<(Option<u64>, bool), ()> {
    let Some(probe) = probe else {
        return Ok((None, kind == Kind::Audio));
    };
    let output = media_command(probe)
        .args([
            "-protocol_whitelist",
            "file,pipe",
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    let data: Value = serde_json::from_slice(&output.stdout).map_err(|_| ())?;
    let streams = data["streams"].as_array().ok_or(())?;
    let audio = streams.iter().any(|stream| stream["codec_type"] == "audio");
    let visual = streams.iter().any(|stream| stream["codec_type"] == "video");
    if (kind == Kind::Audio && !audio) || (kind != Kind::Audio && !visual) {
        return Err(());
    }
    let milliseconds = |v: &Value| {
        v.as_str()?
            .parse::<f64>()
            .ok()
            .filter(|s| s.is_finite() && *s > 0.0)
            .map(|s| (s * 1_000.0).ceil() as u64)
    };
    let duration_ms = if kind == Kind::Image {
        None
    } else {
        milliseconds(&data["format"]["duration"]).or_else(|| {
            streams
                .iter()
                .filter_map(|s| milliseconds(&s["duration"]))
                .max()
        })
    };
    Ok((duration_ms, audio))
}

pub(super) fn find_tool(app_data: &Path, tool: &str) -> Option<PathBuf> {
    let filename = format!("{tool}{}", env::consts::EXE_SUFFIX);
    #[cfg(windows)]
    if let Some(installed) = env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("media-tools").join(&filename)))
        .filter(|path| path.is_file())
    {
        return Some(installed);
    }
    let local = app_data.join("tools").join(&filename);
    if local.is_file() {
        return Some(local);
    }
    let path = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(&filename))
        .find(|binary| binary.is_file())
}

pub(super) fn media_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

pub(super) fn extract_frame(
    ffmpeg: &Path,
    source: &Path,
    at_ms: u64,
    filter: &str,
) -> Result<String, ()> {
    let at = format!("{}.{:03}", at_ms / 1_000, at_ms % 1_000);
    let result = media_command(ffmpeg)
        .args([
            "-nostdin",
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-ss",
            &at,
            "-i",
        ])
        .arg(source)
        .args([
            "-frames:v",
            "1",
            "-vf",
            filter,
            "-threads",
            "1",
            "-f",
            "image2pipe",
            "-c:v",
            "mjpeg",
            "-",
        ])
        .output()
        .map_err(|_| ())?;
    if !result.status.success() || result.stdout.is_empty() {
        return Err(());
    }
    Ok(format!(
        "data:image/jpeg;base64,{}",
        STANDARD.encode(result.stdout)
    ))
}

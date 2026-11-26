use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use uuid::Uuid;

use super::{
    ProjectStore, RenderedSource,
    files::{extract_frame, find_tool, inspect_asset, media_command},
    model::{Asset, Kind, Project},
};

const CPU_H264: &[&str] = &[
    "-c:v",
    "libx264",
    "-threads:v",
    "2",
    "-pix_fmt",
    "yuv420p",
    "-preset",
    "veryfast",
    "-crf",
    "18",
];

#[cfg(windows)]
fn hardware_h264(ffmpeg: &Path) -> Option<&'static [&'static str]> {
    use std::sync::Mutex;

    type Options = &'static [&'static str];
    static CACHE: Mutex<Option<(PathBuf, Option<Options>)>> = Mutex::new(None);
    const CANDIDATES: &[Options] = &[
        &[
            "-c:v",
            "h264_nvenc",
            "-pix_fmt",
            "nv12",
            "-preset",
            "p5",
            "-rc",
            "vbr",
            "-cq",
            "19",
            "-b:v",
            "0",
        ],
        &[
            "-c:v", "h264_amf", "-pix_fmt", "nv12", "-quality", "quality", "-rc", "cqp", "-qp_i",
            "24", "-qp_p", "26", "-qp_b", "27",
        ],
        &[
            "-c:v",
            "h264_qsv",
            "-pix_fmt",
            "nv12",
            "-preset",
            "medium",
            "-global_quality",
            "19",
        ],
    ];
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((path, options)) = cache.as_ref()
        && path == ffmpeg
    {
        return *options;
    }
    let options = CANDIDATES
        .iter()
        .copied()
        .find(|args| super::encoder_probe::supports(ffmpeg, "320x240", args));
    *cache = Some((ffmpeg.to_owned(), options));
    options
}

#[cfg(not(windows))]
fn hardware_h264(_: &Path) -> Option<&'static [&'static str]> {
    None
}

#[derive(PartialEq, Eq, Debug)]
pub(super) struct SourceRange {
    pub asset_id: Uuid,
    pub start: u64,
    pub end: u64,
}

pub(super) struct RenderedClip {
    file: NamedTempFile,
    asset: Asset,
}

impl RenderedClip {
    pub(super) fn insert(
        mut self,
        project: &mut Project,
        media: &Path,
        added: &mut Vec<PathBuf>,
        folder: Option<Uuid>,
    ) -> Result<Uuid, String> {
        project.require_folder(folder)?;
        self.asset.folder_id = folder;
        super::generated::publish(
            project,
            media,
            added,
            self.asset,
            super::generated::Input::Staged(self.file),
        )
    }
}

impl ProjectStore {
    pub(super) fn clip_thumbnail(&self, id: Uuid, at_ms: u64) -> Result<String, String> {
        let project = self.load()?;
        let asset = project.asset(id)?;
        if asset.kind != Kind::Video || asset.duration_ms.is_some_and(|ms| at_ms >= ms) {
            return Err("영상 범위 안의 프레임을 선택해 주세요.".into());
        }
        let ffmpeg = find_tool(&self.app_data, "ffmpeg").ok_or("FFmpeg가 필요합니다.")?;
        let path = asset.path_in(&self.media_dir());
        let filter = "scale=160:96:force_original_aspect_ratio=decrease";
        extract_frame(&ffmpeg, &path, at_ms, filter)
            .or_else(|_| extract_frame(&ffmpeg, &path, at_ms.saturating_sub(250), filter))
            .map_err(|_| "영상 썸네일을 읽을 수 없습니다.".to_owned())
    }

    pub(super) fn save_clip(
        &self,
        scene_id: Uuid,
        expected: u32,
        clip_id: Option<Uuid>,
        folder_id: Option<Uuid>,
    ) -> Result<RenderedSource, String> {
        let project = self.load()?;
        let scene = &project.scenes[project.scene_index(scene_id)?];
        if scene.revision != expected {
            return Err("클립이 변경되었습니다. 다시 선택해 주세요.".into());
        }
        let (asset_id, start, length) = if let Some(layer) = scene
            .settings
            .overlays
            .iter()
            .find(|c| Some(c.id) == clip_id)
        {
            (layer.asset_id, layer.in_ms, layer.duration_ms)
        } else {
            let (clip, duration) =
                if scene.settings.clips.is_empty() && clip_id.is_none_or(|id| id == scene_id) {
                    (
                        scene.settings.visual.ok_or("영상 클립을 선택해 주세요.")?,
                        scene.settings.duration_ms,
                    )
                } else {
                    let selected = scene
                        .settings
                        .clips
                        .iter()
                        .find(|clip| Some(clip.id) == clip_id)
                        .ok_or("영상 클립을 다시 선택해 주세요.")?;
                    (selected.visual, selected.duration_ms)
                };
            (
                clip.source.asset_id,
                clip.source.in_ms,
                clip.trimmed_window(duration)
                    .map_or(duration, |(_, length)| length),
            )
        };
        let asset = project.asset(asset_id)?;
        if asset.kind != Kind::Video {
            return Err("영상 클립을 선택해 주세요.".into());
        }
        let end = start
            .saturating_add(length)
            .min(asset.duration_ms.unwrap_or(u64::MAX));
        if end <= start {
            return Err("저장할 영상 구간이 없습니다.".into());
        }
        let clip = self.render_range(
            &project,
            &SourceRange {
                asset_id,
                start,
                end,
            },
        )?;
        let asset_id = clip.asset.id;
        let view = self.mutate(|project, media, added| {
            clip.insert(project, media, added, folder_id).map(|_| ())
        })?;
        Ok(RenderedSource { view, asset_id })
    }

    pub(super) fn render_range(
        &self,
        project: &Project,
        range: &SourceRange,
    ) -> Result<RenderedClip, String> {
        let SourceRange {
            asset_id,
            start,
            end,
        } = *range;
        let asset = project.asset(asset_id)?;
        let seconds = |ms: u64| format!("{}.{:03}", ms / 1000, ms % 1000);
        let stem = std::path::Path::new(&asset.name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("영상");
        let name = format!("{stem} ({}–{}초).mp4", seconds(start), seconds(end));
        let ffmpeg =
            find_tool(&self.app_data, "ffmpeg").ok_or("클립을 저장하려면 FFmpeg가 필요합니다.")?;
        let probe = find_tool(&self.app_data, "ffprobe");
        let output = tempfile::Builder::new()
            .suffix(".mp4")
            .tempfile_in(self.media_dir())
            .map_err(|_| "임시 영상 파일을 만들 수 없습니다.")?;
        let input_path = asset.path_in(&self.media_dir());
        let encode = |options: &[&str]| -> Result<Asset, String> {
            let result = media_command(&ffmpeg)
                .args([
                    "-nostdin",
                    "-y",
                    "-v",
                    "error",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-ss",
                    &seconds(start),
                    "-i",
                ])
                .arg(&input_path)
                .args([
                    "-t",
                    &seconds(end - start),
                    "-map",
                    "0:v:0",
                    "-map",
                    "0:a:0?",
                    "-vf",
                    "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                    "-filter_threads",
                    "1",
                ])
                .args(options)
                .args(["-c:a", "aac", "-movflags", "+faststart"])
                .arg(output.path())
                .output()
                .map_err(|_| "FFmpeg를 시작할 수 없습니다.")?;
            if !result.status.success() {
                return Err("선택한 구간을 MP4로 저장할 수 없습니다.".into());
            }
            inspect_asset(output.path(), &name, probe.as_deref())
        };
        let accelerated = probe
            .as_ref()
            .and_then(|_| hardware_h264(&ffmpeg))
            .and_then(|options| encode(options).ok())
            .filter(|saved| {
                saved
                    .duration_ms
                    .is_some_and(|ms| ms > 0 && ms.abs_diff(end - start) <= 100)
                    && (!asset.has_audio || saved.has_audio)
            });
        let mut saved = match accelerated {
            Some(saved) => saved,
            None => encode(CPU_H264)?,
        };
        saved.duration_ms = saved.duration_ms.or(Some(end - start));
        if probe.is_none() {
            saved.has_audio = asset.has_audio;
        }
        Ok(RenderedClip {
            file: output,
            asset: saved,
        })
    }
}

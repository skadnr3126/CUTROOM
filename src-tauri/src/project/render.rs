use std::{
    env, io,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(test)]
use std::fs;
use tempfile::NamedTempFile;
#[cfg(test)]
use tempfile::tempdir;
mod bgm_mix;
mod cache;
mod encoder;
mod export;
#[cfg(test)]
mod export_tests;

use encoder::Encoder;
#[cfg(test)]
use export::Export;

use super::{
    composition_layout::{self, CaptionChange, Layout, VisualKeyframe},
    files::media_command,
    model::{Kind, Placement, Project, SceneSettings},
};

#[derive(Clone, Copy, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum PhotoMotion {
    #[default]
    None,
    ZoomIn,
    ZoomOut,
}

#[derive(Clone, Copy, Default, serde::Serialize)]
pub(super) struct SceneStyle<'a> {
    pub layout: Layout,
    pub keyframes: &'a [VisualKeyframe],
    pub side_text: &'a str,
    pub caption_changes: &'a [CaptionChange],
    pub motion: PhotoMotion,
    pub center_text: bool,
}

#[cfg(test)]
pub(super) fn export(
    ffmpeg: &Path,
    media: &Path,
    project: &Project,
    destination: &Path,
) -> Result<(), String> {
    export_range(
        ffmpeg,
        media,
        project,
        destination,
        &[],
        0..project.scenes.len(),
    )
}

#[cfg(test)]
pub(super) fn export_range(
    ffmpeg: &Path,
    media: &Path,
    project: &Project,
    destination: &Path,
    styles: &[SceneStyle],
    range: std::ops::Range<usize>,
) -> Result<(), String> {
    Export {
        ffmpeg,
        media,
        project,
        styles,
        cache_root: None,
    }
    .write_with_encoder(destination, range, Encoder::Cpu)
    .map(|_| ())
}

fn join_segments(
    ffmpeg: &Path,
    work: &Path,
    destination: &Path,
    audio: impl FnOnce(&mut Command) -> Result<(), String>,
) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or("내보내기 경로를 확인해 주세요.")?;
    let result =
        NamedTempFile::new_in(parent).map_err(|_| "선택한 위치에 영상을 저장할 수 없습니다.")?;
    let mut cmd = media_command(ffmpeg);
    cmd.current_dir(work).args([
        "-nostdin",
        "-y",
        "-v",
        "error",
        "-f",
        "concat",
        "-safe",
        "1",
        "-i",
        "segments.txt",
    ]);
    audio(&mut cmd)?;
    run(cmd
        .args(["-movflags", "+faststart", "-f", "mp4"])
        .arg(result.path()))?;
    result
        .as_file()
        .sync_all()
        .map_err(|_| "선택한 위치에 영상을 저장할 수 없습니다.")?;
    result.persist_noclobber(destination).map_err(|error| {
        if error.error.kind() == io::ErrorKind::AlreadyExists {
            "같은 이름의 파일이 있습니다. 새 이름으로 내보내 주세요."
        } else {
            "선택한 위치에 영상을 저장할 수 없습니다."
        }
    })?;
    Ok(())
}

#[cfg(test)]
#[test]
#[ignore = "Requires CUTROOM_TEST_TOOLS with FFmpeg"]
fn join_segments_preserves_existing_files_and_cleans_up_failed_exports() {
    let ffmpeg = PathBuf::from(env::var_os("CUTROOM_TEST_TOOLS").unwrap())
        .join(format!("ffmpeg{}", env::consts::EXE_SUFFIX));
    let work = tempdir().unwrap();
    let folder = tempdir().unwrap();
    let destination = folder.path().join("export.mp4");
    fs::write(&destination, b"existing file").unwrap();
    run(media_command(&ffmpeg).current_dir(work.path()).args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=s=32x32:d=0.1",
        "part.mp4",
    ]))
    .unwrap();
    let manifest = work.path().join("segments.txt");
    fs::write(&manifest, "file 'part.mp4'\n").unwrap();
    let copy = |cmd: &mut Command| {
        cmd.args(["-c", "copy"]);
        Ok(())
    };
    let error = join_segments(&ffmpeg, work.path(), &destination, copy).unwrap_err();
    assert!(error.contains("같은 이름의 파일"));
    assert_eq!(fs::read(&destination).unwrap(), b"existing file");

    fs::write(manifest, "file 'missing.mp4'\n").unwrap();
    assert!(join_segments(&ffmpeg, work.path(), &folder.path().join("new.mp4"), copy).is_err());
    assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
}

fn scene_command(
    ffmpeg: &Path,
    work: &Path,
    media: &Path,
    project: &Project,
    scene: &SceneSettings,
    font: bool,
    style: SceneStyle,
    output: &str,
    ass_offset: Option<u64>,
    encoder: Encoder,
) -> Result<Command, String> {
    let duration = seconds(scene.duration_ms);
    let trim = scene
        .visual
        .and_then(|clip| clip.trimmed_window(scene.duration_ms));
    let visual_position = scene
        .visual
        .filter(|_| trim.is_none_or(|(_, length)| length > 0))
        .map(|clip| clip.source);
    let trim = trim.filter(|(_, length)| *length > 0);
    let visual = visual_position
        .map(|source| project.asset(source.asset_id))
        .transpose()?;
    let natural_audio = visual_position
        .filter(|_| scene.visual.is_none_or(|v| !v.muted))
        .filter(|_| visual.is_some_and(|asset| asset.kind == Kind::Video && asset.has_audio));
    let audio_trim = if scene.audio.is_none() {
        natural_audio.zip(trim)
    } else {
        None
    };
    let mut command = media_command(ffmpeg);
    command.current_dir(work).args(["-nostdin", "-v", "error"]);
    if let Some(asset) = visual {
        input(
            &mut command,
            &asset.path_in(media),
            visual_position.map_or(0, |source| source.in_ms),
        );
    } else {
        command.args(["-f", "lavfi", "-i", "color=c=0x151820:s=1280x720:r=30"]);
    }
    if let Some(Placement { asset_id, in_ms }) = scene.audio.or(natural_audio)
        && !project
            .asset(asset_id)?
            .duration_ms
            .is_some_and(|duration| in_ms >= duration)
    {
        let asset = project.asset(asset_id)?;
        let seek = if audio_trim.is_some() { 0 } else { in_ms };
        input(&mut command, &asset.path_in(media), seek);
    } else {
        command.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"]);
    }

    let visual_duration = trim.map_or_else(|| duration.clone(), |(_, length)| seconds(length));
    let (width, height) = if style.keyframes.is_empty() {
        style.layout.visual_size()
    } else {
        (1280, 720)
    };
    let mut video_filter = format!(
        concat!(
            "[0:v:0]scale={width}:{height}:force_original_aspect_ratio=decrease:force_divisible_by=2,",
            "pad=w={width}:h={height}:x=(ow-iw)/2:y=(oh-ih)/2:color=0x151820,setsar=1,fps=30,",
            "tpad=stop_mode=clone:stop_duration={length},trim=duration={length},setpts=PTS-STARTPTS"
        ),
        length = visual_duration,
        width = width,
        height = height,
    );
    if style.motion != PhotoMotion::None {
        let last_frame = (scene.duration_ms * 30)
            .div_ceil(1000)
            .saturating_sub(1)
            .max(1);
        let zoom = match style.motion {
            PhotoMotion::ZoomIn => format!("1+0.2*on/{last_frame}"),
            PhotoMotion::ZoomOut => format!("1.2-0.2*on/{last_frame}"),
            PhotoMotion::None => unreachable!(),
        };
        video_filter.push_str(&format!(
            ",zoompan=z='{zoom}':x='iw/2-iw/zoom/2':y='ih/2-ih/zoom/2':d=1:s={width}x{height}:fps=30"
        ));
    }
    if let Some((lead, length)) = trim {
        let tail = scene.duration_ms - lead - length;
        video_filter.push_str(&format!(
            ",tpad=start_duration={}:stop_duration={}:color=0x151820,trim=duration={duration}",
            seconds(lead),
            seconds(tail),
        ));
    }
    let font = if font {
        "fontfile=caption.ttf:"
    } else {
        "font=Sans:"
    };
    style.layout.compose(
        &mut video_filter,
        style.side_text,
        style.keyframes,
        work,
        font,
        scene.duration_ms,
    )?;
    if let Some(frame) = scene.visual.and_then(|v| v.frame) {
        let w = 1280 * u32::from(frame.size) / 100 / 2 * 2;
        let h = 720 * u32::from(frame.size) / 100 / 2 * 2;
        video_filter.push_str(&format!(
            ",scale={w}:{h},pad=1280:720:(ow-iw)*{}/100:(oh-ih)*{}/100:color=0x151820",
            frame.x, frame.y
        ));
    }
    let mut extra_audio = Vec::new();
    for (index, layer) in scene.overlays.iter().enumerate() {
        let asset = project.asset(layer.asset_id)?;
        if asset.kind == Kind::Image {
            command.args(["-loop", "1", "-framerate", "30"]);
        }
        input(
            &mut command,
            &asset.path_in(media),
            if asset.kind == Kind::Image {
                0
            } else {
                layer.in_ms
            },
        );
        let w = 1280 * u32::from(layer.size) / 100;
        let h = 720 * u32::from(layer.size) / 100;
        let start = seconds(layer.start_ms);
        let end = seconds(layer.start_ms + layer.duration_ms);
        let length = seconds(layer.duration_ms);
        let stream = index + 2;
        if layer.sound && asset.has_audio && scene.audio.is_none() {
            extra_audio.push((stream, length.clone(), layer.start_ms));
        }
        video_filter.push_str(&format!(
            "[bg{index}];[{stream}:v]fps=30,scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2,setsar=1,trim=duration={length},setpts=PTS-STARTPTS+{start}/TB[fg{index}];[bg{index}][fg{index}]overlay=x=(W-{w})*{x}/100+({w}-w)/2:y=(H-{h})*{y}/100+({h}-h)/2:enable='gte(t,{start})*lt(t,{end})'",
            x=layer.x,y=layer.y
        ));
    }
    if let Some(offset) = ass_offset {
        video_filter.push(',');
        video_filter.push_str(&super::ass::render_filter(offset * 30, false));
    } else {
        let timed_changes = super::caption_track::changes(&scene.caption_cues);
        composition_layout::captions(
            &mut video_filter,
            work,
            font,
            &scene.caption,
            if timed_changes.is_empty() {
                style.caption_changes
            } else {
                &timed_changes
            },
            style.center_text,
        )?;
    }
    let mut audio_filter = String::from("[1:a:0]aresample=48000,aformat=channel_layouts=stereo");
    if let Some((source, (lead, length))) = audio_trim {
        audio_filter.push_str(&format!(
            ",atrim=start={}:end={},asetpts=N/SR/TB",
            seconds(source.in_ms),
            seconds(source.in_ms.saturating_add(length)),
        ));
        if lead > 0 {
            audio_filter.push_str(&format!(
                "[clip_audio];anullsrc=r=48000:cl=stereo,atrim=duration={}[lead_audio];[lead_audio][clip_audio]concat=n=2:v=0:a=1",
                seconds(lead),
            ));
        }
    }
    if !extra_audio.is_empty() {
        audio_filter.push_str(",apad,atrim=duration=");
        audio_filter.push_str(&duration);
        audio_filter.push_str("[bed];");
        let mut inputs = String::from("[bed]");
        for (index, length, delay) in &extra_audio {
            audio_filter.push_str(&format!("[{index}:a:0]aresample=48000,aformat=channel_layouts=stereo,atrim=duration={length},asetpts=PTS-STARTPTS,adelay={delay}:all=1[a{index}];"));
            inputs.push_str(&format!("[a{index}]"));
        }
        audio_filter.push_str(&format!(
            "{inputs}amix=inputs={}:normalize=0",
            extra_audio.len() + 1
        ));
    }
    let filters = format!(
        concat!(
            "{video_filter}[video];{audio_filter}",
            ",apad,atrim=duration={duration},asetpts=PTS-STARTPTS[audio]"
        ),
        video_filter = video_filter,
        audio_filter = audio_filter,
        duration = duration,
    );
    command.args([
        "-filter_complex_threads",
        "2",
        "-filter_complex",
        &filters,
        "-map",
        "[video]",
        "-map",
        "[audio]",
        "-t",
        &duration,
    ]);
    encoder.add_output_args(&mut command);
    command.args(["-c:a", "aac", "-b:a", "160k", "-ar", "48000", output]);
    Ok(command)
}

fn input(command: &mut Command, path: &Path, in_ms: u64) {
    command.args(["-protocol_whitelist", "file,pipe"]);
    if in_ms > 0 {
        command.args(["-ss", &seconds(in_ms)]);
    }
    command.arg("-i").arg(path);
}

fn run(command: &mut Command) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|_| "FFmpeg를 실행할 수 없습니다.")?;
    if output.status.success() {
        return Ok(());
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    let detail = diagnostic
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("\n");
    Err(if detail.is_empty() {
        "FFmpeg가 작업을 완료하지 못했습니다.".to_owned()
    } else {
        detail
    })
}

fn seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}

pub(super) fn caption_font() -> Option<PathBuf> {
    let windows = env::var_os("WINDIR").map(PathBuf::from);
    let font = windows.as_ref().map(|root| root.join("Fonts/malgun.ttf"));
    [
        font,
        Some(PathBuf::from(
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        )),
        Some(PathBuf::from("/System/Library/Fonts/AppleSDGothicNeo.ttc")),
    ]
    .into_iter()
    .flatten()
    .find(|font| font.is_file())
}

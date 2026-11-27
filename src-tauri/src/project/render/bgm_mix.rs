use super::{Project, media_command, run, seconds};
use std::{ops::Range, path::Path, process::Command};

/// Add a single continuous mix to the assembled soundtrack. A scene-only export
/// intersects the same global envelope, including its position within the song.
pub(super) fn configure(
    command: &mut Command,
    media: &Path,
    project: &Project,
    window: Range<u64>,
) -> Result<(), String> {
    let mut filters = Vec::new();
    let mut inputs = String::from("[0:a]");
    for clip in &project.bgm {
        let start = project
            .anchor_time(clip.start)
            .ok_or("BGM 시작 구간을 찾을 수 없습니다.")?;
        let anchor_end = project
            .anchor_time(clip.end)
            .ok_or("BGM 끝 구간을 찾을 수 없습니다.")?;
        let source = project.assets.iter().find(|a| a.id == clip.asset_id);
        let end = clip.playback_end(start, anchor_end, source.and_then(|a| a.duration_ms));
        let left = start.max(window.start);
        let right = end.min(window.end);
        if left >= right || clip.volume == 0 {
            continue;
        }
        let source = source.ok_or("BGM 소스를 찾을 수 없습니다.")?;
        let mut input = source.path_in(media);
        if clip.source_duration_ms.is_some() {
            if let Some(range) = clip.source_range {
                command.args(["-ss", &seconds(range.in_ms)]);
            }
        } else if let Some(range) = clip.source_range {
            if range.in_ms >= range.out_ms {
                return Err("BGM 원본 범위를 확인해 주세요.".into());
            }
            // Stage only this excerpt on disk so looping long songs does not require a PCM buffer in RAM.
            let cut = command
                .get_current_dir()
                .ok_or("내보내기 작업 폴더가 없습니다.")?
                .join(format!("bgm-{}.flac", clip.id));
            let ffmpeg = Path::new(command.get_program());
            run(media_command(ffmpeg)
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-ss",
                    &seconds(range.in_ms),
                    "-i",
                ])
                .arg(&input)
                .args([
                    "-t",
                    &seconds(range.out_ms - range.in_ms),
                    "-map",
                    "0:a:0",
                    "-ar",
                    "48000",
                    "-c:a",
                    "flac",
                ])
                .arg(&cut))?;
            input = cut;
        }
        if clip.source_duration_ms.is_none() {
            command.args(["-stream_loop", "-1"]);
        }
        command.arg("-i").arg(input);
        let i = filters.len() + 1;
        let length = end - start;
        let mut filter = format!(
            "[{i}:a]aresample=48000,aformat=channel_layouts=stereo,asetpts=N/SR/TB,volume={}",
            f64::from(clip.volume) / 100.0
        );
        let fade_in = clip.fade_in_ms.min(length);
        let fade_out = clip.fade_out_ms.min(length);
        if fade_in > 0 {
            filter.push_str(&format!(",afade=t=in:d={}", seconds(fade_in)));
        }
        if fade_out > 0 {
            filter.push_str(&format!(
                ",afade=t=out:st={}:d={}",
                seconds(length - fade_out),
                seconds(fade_out)
            ));
        }
        filter.push_str(&format!(
            ",atrim=start={}:end={},asetpts=N/SR/TB,adelay={}:all=1[bgm{i}]",
            seconds(left - start),
            seconds(right - start),
            left - window.start
        ));
        filters.push(filter);
        inputs.push_str(&format!("[bgm{i}]"));
    }
    if filters.is_empty() {
        command.args(["-c", "copy"]);
    } else {
        filters.push(format!(
            "{inputs}amix=inputs={}:duration=first:normalize=0[mix]",
            filters.len() + 1
        ));
        command.args([
            "-filter_complex",
            &filters.join(";"),
            "-map",
            "0:v:0",
            "-map",
            "[mix]",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
        ]);
    }
    Ok(())
}

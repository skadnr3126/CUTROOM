use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

const BACKGROUND: &str = "0x151820";

#[derive(Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Layout {
    #[default]
    Full,
    ImageLeft,
    ImageRight,
    SlideToLeft,
    SlideToRight,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VisualKeyframe {
    time_ms: u64,
    center_x: f64,
    center_y: f64,
    scale_percent: f64,
}

pub(super) fn validate_motion(keys: &[VisualKeyframe], duration: u64) -> Result<(), String> {
    for (i, key) in keys.iter().enumerate() {
        if (i == 0 && key.time_ms != 0)
            || (i > 0 && key.time_ms <= keys[i - 1].time_ms)
            || key.time_ms > duration
            || !(0.0..=100.0).contains(&key.center_x)
            || !(0.0..=100.0).contains(&key.center_y)
            || !(5.0..=200.0).contains(&key.scale_percent)
        {
            return Err("사진의 시작 위치, 배율 또는 이동 시각을 확인해 주세요.".into());
        }
    }
    Ok(())
}

fn eased_progress(start: f64, span: f64) -> String {
    format!("(0.5-0.5*cos(PI*clip((t-{start:.6})/{span:.6},0,1)))")
}

// Each ramp contributes only the change between two neighboring keyframes.
fn interpolate(keys: &[VisualKeyframe], value: fn(&VisualKeyframe) -> f64) -> String {
    let mut expression = format!("{:.6}", value(&keys[0]));
    for pair in keys.windows(2) {
        let delta = value(&pair[1]) - value(&pair[0]);
        if delta != 0.0 {
            let start = pair[0].time_ms as f64 / 1000.0;
            let span = (pair[1].time_ms - pair[0].time_ms) as f64 / 1000.0;
            let progress = eased_progress(start, span);
            expression.push_str(&format!("+({delta:.6})*{progress}"));
        }
    }
    expression
}

impl Layout {
    pub fn visual_size(self) -> (u32, u32) {
        if self == Self::Full {
            (1280, 720)
        } else {
            (560, 544)
        }
    }

    pub fn compose(
        self,
        filter: &mut String,
        side_text: &str,
        keys: &[VisualKeyframe],
        work: &Path,
        font: &str,
        duration_ms: u64,
    ) -> Result<(), String> {
        if self == Self::Full && keys.is_empty() {
            return Ok(());
        }
        let left = matches!(self, Self::ImageLeft | Self::SlideToLeft);
        let moving = matches!(self, Self::SlideToLeft | Self::SlideToRight);
        let dest_x = if left { 48 } else { 672 };
        let seconds = duration_ms as f64 / 1000.0;
        let hold = seconds * 0.30;
        let travel = seconds * 0.14;
        let x = if moving {
            format!("360+({dest_x}-360)*{}", eased_progress(hold, travel))
        } else {
            dest_x.to_string()
        };
        let (x, y) = if keys.is_empty() {
            (x, "56".to_owned())
        } else {
            let scale = interpolate(keys, |k| k.scale_percent);
            filter.push_str(&format!(
                ",scale=w='2*round(iw*({scale})/200)':h='2*round(ih*({scale})/200)':eval=frame"
            ));
            let cx = interpolate(keys, |k| k.center_x);
            let cy = interpolate(keys, |k| k.center_y);
            (format!("W*({cx})/100-w/2"), format!("H*({cy})/100-h/2"))
        };
        filter.push_str(&format!(
            "[panel];color=c={BACKGROUND}:s=1280x720:r=30[bg];[bg][panel]overlay=x='{x}':y='{y}':shortest=1"
        ));

        // Explicit line breaks are reveal points. A wrapped sentence appears as one item.
        let items: Vec<_> = side_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| wrap_text(line, 33))
            .collect();
        let rows: usize = items.iter().map(|line| line.lines().count()).sum();
        if rows > 15 {
            return Err("옆 설명이 한 화면을 넘습니다. 설명을 여러 컷으로 나눠 주세요.".into());
        }
        let mut y = 56 + (544 - rows as i32 * 36) / 2;
        let text_x = if left { 664 } else { 48 };
        let reveal = if moving { hold + travel } else { 0.0 };
        for (i, item) in items.iter().enumerate() {
            let name = format!("side-{i}.txt");
            write_text(work, &name, item)?;
            let start = reveal + i as f64 * (seconds - reveal) * 0.65 / items.len() as f64;
            let fade = (seconds * 0.05).min(0.20);
            let color = if i == 0 { "0xffdf72" } else { "white" };
            filter.push_str(&format!(
                ",drawtext={font}textfile={name}:expansion=none:fontsize=28:fontcolor={color}:line_spacing=5:x={text_x}:y={y}:alpha='clip((t-{start:.6})/{fade:.6},0,1)'"
            ));
            y += item.lines().count() as i32 * 36;
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CaptionChange {
    pub start_ms: u64,
    pub text: String,
}

pub(super) fn captions(
    filter: &mut String,
    work: &Path,
    font: &str,
    first: &str,
    changes: &[CaptionChange],
    center: bool,
) -> Result<(), String> {
    let y = if center {
        "(h-th)/2"
    } else {
        "max(20\\,h-th-55)"
    };
    for (i, (start, text)) in std::iter::once((0, first))
        .chain(changes.iter().map(|c| (c.start_ms, c.text.as_str())))
        .enumerate()
    {
        if text.trim().is_empty() {
            continue;
        }
        let name = format!("caption-{i}.txt");
        write_text(work, &name, &wrap_text(text, 58))?;
        filter.push_str(&format!(
            ",drawtext={font}textfile={name}:expansion=none:fontsize=32:fontcolor=white:box=1:boxcolor=black@0.78:boxborderw=14:line_spacing=7:x=(w-tw)/2:y={y}"
        ));
        if !changes.is_empty() {
            let start = start as f64 / 1000.0;
            let end = changes
                .get(i)
                .map(|c| format!("*lt(t,{:.3})", c.start_ms as f64 / 1000.0))
                .unwrap_or_default();
            filter.push_str(&format!(":enable='gte(t,{start:.3}){end}'"));
        }
    }
    Ok(())
}

fn write_text(work: &Path, name: &str, text: &str) -> Result<(), String> {
    fs::write(work.join(name), text).map_err(|_| "영상에 표시할 문구를 저장할 수 없습니다.".into())
}

fn wrap_text(text: &str, max_width: usize) -> String {
    let mut wrapped = String::new();
    let mut width = 0;
    for character in text.chars() {
        if character == '\r' {
            continue;
        }
        if character == '\n' {
            wrapped.push('\n');
            width = 0;
            continue;
        }
        let units = if character.is_ascii() { 1 } else { 2 };
        if width + units > max_width {
            wrapped.push('\n');
            width = 0;
        }
        wrapped.push(character);
        width += units;
    }
    wrapped
}

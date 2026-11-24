use super::{ProjectStore, files, run_io};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::path::Path;

pub(super) fn sheet(
    ffmpeg: &Path,
    source: &str,
    first: u64,
    count: u32,
) -> Result<Vec<u8>, String> {
    if source.len() > 2_000_000
        || !(1..=15).contains(&count)
        || first > u64::from(u32::MAX) * 108000
    {
        return Err("자막 미리보기 범위를 확인해 주세요.".into());
    }
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    std::fs::write(temp.path().join("scene.ass"), source).map_err(|e| e.to_string())?;
    let cols = count.min(5);
    let rows = count.div_ceil(cols);
    // The same executable, font provider, resolution and scene clock as the MP4 compositor.
    let subtitles = super::ass::render_filter(first * 1000, true);
    let filter = format!(
        "{subtitles},format=gbrap,unpremultiply=inplace=1,format=rgba,tile={cols}x{rows}:nb_frames={count}"
    );
    let output = files::media_command(ffmpeg)
        .current_dir(temp.path())
        .args([
            "-nostdin",
            "-v",
            "error",
            "-filter_threads",
            "2",
            "-f",
            "lavfi",
            "-i",
            "color=black@0:s=1280x720:r=30,format=yuva420p",
            "-vf",
            &filter,
            "-frames:v",
            "1",
            "-threads",
            "2",
            "-compression_level",
            "1",
            "-c:v",
            "png",
            "-f",
            "image2pipe",
            "pipe:1",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err(format!(
            "자막 프레임을 그릴 수 없습니다. {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

#[tauri::command]
pub(crate) async fn ass_preview_sheet(
    store: ProjectStore,
    source: String,
    first: u64,
    count: u32,
) -> Result<String, String> {
    let root = store.app_data.clone();
    run_io(move || {
        let ff = files::find_tool(&root, "ffmpeg").ok_or("FFmpeg를 찾을 수 없습니다.")?;
        Ok(STANDARD.encode(sheet(&ff, &source, first, count)?))
    })
    .await
}

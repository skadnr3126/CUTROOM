use super::run_io;
use std::{fs, path::Path};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub(crate) async fn ass_save_file(
    app: AppHandle,
    track: super::ass::AssTrack,
) -> Result<bool, String> {
    run_io(move || {
        use std::io::Write;
        track.validate()?;
        let name = std::path::Path::new(&track.name)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("자막");
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("ASS 자막", &["ass"])
            .set_file_name(format!("{name} 편집본.ass"))
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = file
            .into_path()
            .map_err(|_| "저장 위치를 확인해 주세요.")?
            .with_extension("ass");
        let mut output =
            tempfile::NamedTempFile::new_in(path.parent().ok_or("저장 폴더를 찾을 수 없습니다.")?)
                .map_err(|e| e.to_string())?;
        output
            .write_all(track.source.as_bytes())
            .map_err(|e| e.to_string())?;
        output.as_file().sync_all().map_err(|e| e.to_string())?;
        output.persist(path).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
}

pub(super) fn read_path(path: &Path) -> Result<super::ass::AssTrack, String> {
    if !path.is_absolute()
        || path
            .extension()
            .and_then(|v| v.to_str())
            .is_none_or(|v| !v.eq_ignore_ascii_case("ass"))
    {
        return Err("ASS 파일의 전체 경로를 지정해 주세요.".into());
    }
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > 2_000_000 {
        return Err("ASS 파일은 2MB까지 불러올 수 있습니다.".into());
    }
    Ok(super::ass::AssTrack {
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        source: fs::read_to_string(path).map_err(|_| "UTF-8 ASS 파일을 읽을 수 없습니다.")?,
    })
}

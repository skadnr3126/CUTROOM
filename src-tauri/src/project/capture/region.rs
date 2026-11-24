use super::super::{ProjectStore, RenderedSource};
use tauri::WebviewWindow;
#[cfg(windows)]
use {
    super::super::{files, run_io},
    super::{register, staged_file},
    std::{io::Write, path::Path},
    tempfile::NamedTempFile,
};

#[tauri::command]
pub(crate) async fn capture_region(
    window: WebviewWindow,
    store: ProjectStore,
    folder_id: Option<uuid::Uuid>,
) -> Result<Option<RenderedSource>, String> {
    #[cfg(not(windows))]
    {
        let _ = (window, store, folder_id);
        Err("영역 캡처는 Windows에서 사용할 수 있습니다.".into())
    }
    #[cfg(windows)]
    {
        window.hide().map_err(|_| "컷룸 창을 숨길 수 없습니다.")?;
        let result = run_io(move || {
            let output = staged_file(&store, ".png")?;
            if !select_region(output.path())? {
                return Ok(None);
            }
            register(&store, output, "화면 캡처.png", folder_id, false).map(Some)
        })
        .await;
        window
            .show()
            .map_err(|_| "컷룸 창을 다시 표시할 수 없습니다.")?;
        let _ = window.set_focus();
        result
    }
}

#[cfg(windows)]
fn select_region(output: &Path) -> Result<bool, String> {
    let mut source = NamedTempFile::new().map_err(|_| "영역 선택기를 준비할 수 없습니다.")?;
    source
        .write_all(include_bytes!("RegionPicker.cs"))
        .map_err(|_| "영역 선택기를 준비할 수 없습니다.")?;
    let source = source.into_temp_path();
    let shell = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .ok_or("Windows 경로를 찾을 수 없습니다.")?
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let status = files::media_command(&shell)
        .args(["-NoProfile", "-NonInteractive", "-STA", "-Command",
            "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition ([IO.File]::ReadAllText($env:CUTROOM_PICKER)) -ReferencedAssemblies System.Windows.Forms,System.Drawing; exit ([Cutroom.RegionPicker]::SelectRegion($env:CUTROOM_IMAGE))"])
        .env("CUTROOM_PICKER", &*source).env("CUTROOM_IMAGE", output)
        .status().map_err(|_| "Windows 영역 선택기를 열 수 없습니다.")?;
    match status.code() {
        Some(0) => Ok(true),
        Some(2) => Ok(false),
        _ => Err("화면 영역을 캡처할 수 없습니다.".into()),
    }
}

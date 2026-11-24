pub(crate) mod recording;
pub(crate) mod region;

use super::{ProjectStore, RenderedSource, files};
use tempfile::NamedTempFile;

fn register(
    store: &ProjectStore,
    output: NamedTempFile,
    name: &str,
    folder_id: Option<uuid::Uuid>,
    has_audio: bool,
) -> Result<RenderedSource, String> {
    let probe = files::find_tool(&store.app_data, "ffprobe");
    let mut asset = files::inspect_asset(output.path(), name, probe.as_deref())?;
    if probe.is_none() {
        asset.has_audio = has_audio;
    }
    let asset_id = asset.id;
    let view = store.mutate(|project, media, added| {
        // A recording may finish after its destination folder has been removed.
        asset.folder_id = folder_id.filter(|id| project.folders.iter().any(|f| f.id == *id));
        super::generated::publish(
            project,
            media,
            added,
            asset,
            super::generated::Input::Staged(output),
        )?;
        Ok(())
    })?;
    Ok(RenderedSource { view, asset_id })
}

fn staged_file(store: &ProjectStore, suffix: &str) -> Result<NamedTempFile, String> {
    let media = store.media_dir();
    std::fs::create_dir_all(&media).map_err(|_| "소스 보관함을 열 수 없습니다.")?;
    tempfile::Builder::new()
        .prefix(".capture-")
        .suffix(suffix)
        .tempfile_in(media)
        .map_err(|_| "캡처 파일을 준비할 수 없습니다.".into())
}

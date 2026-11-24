//! Publish finished media and bind retries to the original asset ID.
use super::{
    asset_filename::filename,
    files, media_tree,
    model::{Asset, Kind, Project},
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use uuid::Uuid;
#[cfg(test)]
mod tests;

pub(super) fn valid_assets(project: &Project) -> bool {
    let mut seen = HashSet::new();
    project.assets.iter().all(|a| {
        Kind::from_extension(&a.extension) == Some(a.kind)
            && media_tree::valid_asset(a)
            && (a.missing || seen.insert(a.project_relative_file().to_lowercase()))
    })
}

pub(super) fn named(title: &str, ext: &str, number: usize) -> String {
    let title = title.trim();
    let stem = title
        .rsplit_once('.')
        .filter(|(_, suffix)| suffix.eq_ignore_ascii_case(ext))
        .map_or(title, |(stem, _)| stem);
    filename(
        &format!("{}.{}", if stem.is_empty() { "소스" } else { stem }, ext),
        number,
    )
}

pub(super) enum Input<'a> {
    Staged(NamedTempFile),
    File(&'a Path),
}

pub(super) fn publish(
    p: &mut Project,
    media: &Path,
    added: &mut Vec<PathBuf>,
    mut a: Asset,
    input: Input<'_>,
) -> Result<Uuid, String> {
    let parent = media_tree::folder_path(p, a.folder_id)?;
    let root = media.parent().ok_or("프로젝트 경로를 찾을 수 없습니다.")?;
    let dir = media_tree::checked(root, &parent)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = (1..)
        .map(|n| dir.join(named(&a.name, &a.extension, n)))
        .find(|path| !path.exists() && !p.assets.iter().any(|other| other.path_in(media) == *path))
        .ok_or("파일 이름을 정할 수 없습니다.")?;
    let file = match input {
        Input::Staged(f) => f,
        Input::File(path) => files::stage_copy(path, &dir, &a.name)?,
    };
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist_noclobber(&target).map_err(|e| e.to_string())?;
    added.push(target.clone());
    a.project_file = Some(format!(
        "{parent}/{}",
        target.file_name().unwrap().to_string_lossy()
    ));
    let id = a.id;
    p.assets.push(a);
    Ok(id)
}

impl super::ProjectStore {
    pub(super) fn register_output(
        &self,
        job: Uuid,
        path: &Path,
        title: Option<&str>,
        folder: Option<Uuid>,
    ) -> Result<Asset, String> {
        // Files written directly into media may already have been discovered by the editor.
        self.sync_media()?;
        let mut id = Uuid::nil();
        let p = self.commit(|project, media, added| {
            if let Some(asset) = project.assets.iter().find(|a| a.source_job_id == Some(job)) {
                id = asset.id;
                return Ok(());
            }
            project.require_folder(folder)?;
            if !path.is_absolute() || !path.is_file() {
                return Err("완성된 파일의 절대 경로를 선택해 주세요.".into());
            }
            let title = title.map(str::trim);
            if title.is_some_and(|value| value.is_empty() || value.chars().any(char::is_control)) {
                return Err("소스 제목을 입력해 주세요.".into());
            }
            if let Some(asset) = media_tree::registered_file(project, media, path)? {
                if asset.source_job_id.is_some() {
                    return Err("다른 작업에 이미 등록된 파일입니다.".into());
                }
                asset.source_job_id = Some(job);
                if folder.is_some() { asset.folder_id = folder; }
                if let Some(title) = title { asset.name = title.to_owned(); }
                id = asset.id;
            } else {
                let name = path.file_name().and_then(|s|s.to_str()).ok_or("파일 이름을 읽을 수 없습니다.")?;
                let probe = files::find_tool(&self.app_data, "ffprobe").ok_or("ffprobe를 찾을 수 없습니다.")?;
                let mut asset = files::inspect_asset(path, name, Some(&probe))?;
                asset.source_job_id = Some(job);
                asset.folder_id = folder;
                if let Some(title) = title { asset.name = title.to_owned(); }
                id = publish(project, media, added, asset, Input::File(path))?;
            }
            Ok(())
        })?;
        Ok(p.asset(id)?.clone())
    }
}

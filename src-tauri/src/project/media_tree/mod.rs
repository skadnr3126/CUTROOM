//! On-disk paths own the bin layout. Scene references continue to use stable asset IDs.
mod journal;
mod scan;
#[cfg(test)]
mod tests;
use super::{Project, ProjectStore, asset_filename::filename, generated, model::Asset};
pub(super) use journal::{Journal, recover};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const ARCHIVE: &str = ".cutroom-retained";
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DiskStamp {
    key: String,
    size: u64,
    modified: u128,
}

pub(super) fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
pub(super) fn safe_component(name: &str) -> bool {
    !name.is_empty() && filename(name, 1) == name && name != "." && name != ".."
}
pub(super) fn checked(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut parts = relative.split('/');
    let base = parts.next().unwrap_or_default();
    if !matches!(base, "media" | ".cutroom-retained" | "생성 영상") {
        return Err("잘못된 미디어 경로입니다.".into());
    }
    let mut path = root.to_owned();
    for part in std::iter::once(base).chain(parts) {
        if !safe_component(part) {
            return Err(format!("사용할 수 없는 경로입니다. {relative}"));
        }
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(m) if linked(&m) => {
                return Err(format!(
                    "연결된 폴더나 파일을 변경할 수 없습니다. {relative}"
                ));
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(path)
}
pub(super) fn valid_asset(a: &Asset) -> bool {
    let Some(rel) = &a.project_file else {
        return true;
    };
    let Some((base, tail)) = rel.split_once('/') else {
        return false;
    };
    let archive =
        base == ARCHIVE && a.removed_from_bin && tail == format!("{}.{}", a.id, a.extension);
    (archive || base == "media" || (base == "생성 영상" && !tail.contains('/')))
        && tail.split('/').all(safe_component)
        && tail
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(&a.extension))
}
fn folder_paths(p: &Project) -> Result<HashMap<Uuid, String>, String> {
    let all: HashMap<_, _> = p.folders.iter().map(|f| (f.id, f)).collect();
    let mut result = HashMap::new();
    for f in &p.folders {
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        let mut id = Some(f.id);
        while let Some(i) = id {
            let node = all.get(&i).ok_or("상위 폴더를 찾을 수 없습니다.")?;
            if !seen.insert(i) || !safe_component(&node.name) {
                return Err(format!("잘못된 보관함 폴더입니다. {}", node.name));
            }
            names.push(node.name.as_str());
            id = node.parent_id;
        }
        names.reverse();
        result.insert(f.id, format!("media/{}", names.join("/")));
    }
    Ok(result)
}
pub(super) fn folder_path(p: &Project, id: Option<Uuid>) -> Result<String, String> {
    match id {
        Some(i) => folder_paths(p)?
            .remove(&i)
            .ok_or("폴더를 찾을 수 없습니다.".into()),
        None => Ok("media".into()),
    }
}

pub(super) fn registered_file<'a>(project: &'a mut Project,media:&Path,path:&Path)->Result<Option<&'a mut Asset>,String>{
    let path=dunce::canonicalize(path).map_err(|e|e.to_string())?;
    let directory=dunce::canonicalize(media).map_err(|e|e.to_string())?;
    let Ok(relative)=path.strip_prefix(directory) else{return Ok(None)};
    let relative=format!("media/{}",relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR,"/")).to_lowercase();
    project.assets.iter_mut().find(|asset|!asset.missing && !asset.removed_from_bin && asset.project_relative_file().to_lowercase()==relative)
        .map(Some).ok_or_else(||"파일이 아직 보관함에 준비되지 않았습니다. 저장이 끝난 뒤 다시 시도해 주세요.".into())
}

pub(super) struct Layout {
    folders: HashMap<Uuid, String>,
    assets: HashMap<Uuid, (Option<Uuid>, String, bool)>,
}
impl Layout {
    pub fn capture(p: &Project) -> Result<Self, String> {
        Ok(Self {
            folders: folder_paths(p)?,
            assets: p
                .assets
                .iter()
                .map(|a| (a.id, (a.folder_id, a.name.clone(), a.removed_from_bin)))
                .collect(),
        })
    }
}
fn translate(path: &str, moves: &[(String, String)]) -> String {
    let mut current = path.to_owned();
    for (old, new) in moves {
        if &current == old {
            current = new.clone()
        } else if let Some(rest) = current.strip_prefix(&format!("{old}/")) {
            current = format!("{new}/{rest}");
        }
    }
    current
}
fn unique(
    root: &Path,
    folder: &str,
    asset: &Asset,
    reserved: &HashSet<String>,
    old: &str,
) -> Result<String, String> {
    for n in 1.. {
        let candidate = format!(
            "{folder}/{}",
            generated::named(&asset.name, &asset.extension, n)
        );
        if candidate.eq_ignore_ascii_case(old)
            || (!reserved.contains(&candidate.to_lowercase())
                && !checked(root, &candidate)?.exists())
        {
            return Ok(candidate);
        }
    }
    unreachable!()
}

pub(super) fn materialize(
    root: &Path,
    p: &mut Project,
    before: &Layout,
    j: &mut Journal,
    migrate: bool,
) -> Result<(), String> {
    let mut moves = Vec::new();
    if migrate {
        let legacy = root.join("생성 영상");
        if legacy.is_dir() {
            if fs::read_dir(&legacy)
                .map_err(|e| e.to_string())?
                .next()
                .is_none()
                && !root.join("media/생성 영상").exists()
            {
                j.remove_empty("생성 영상")?;
                if let Some(f) = p
                    .folders
                    .iter()
                    .find(|f| f.parent_id.is_none() && f.name == "생성 영상")
                {
                    let id = f.id;
                    if !p.assets.iter().any(|a| a.folder_id == Some(id))
                        && !p.folders.iter().any(|f| f.parent_id == Some(id))
                    {
                        p.folders.retain(|f| f.id != id);
                    }
                }
            } else if !root.join("media/생성 영상").exists() {
                j.relocate("생성 영상", "media/생성 영상")?;
                moves.push(("생성 영상".into(), "media/생성 영상".into()));
            }
        }
    }
    let paths = folder_paths(p)?;
    let mut ordered: Vec<_> = paths.iter().collect();
    ordered.sort_by_key(|(_, path)| path.matches('/').count());
    for (id, new) in ordered {
        if let Some(old) = before.folders.get(id) {
            let current = translate(old, &moves);
            if current != *new && checked(root, &current)?.is_dir() {
                if current.eq_ignore_ascii_case(new) {
                    let temp = format!(
                        "{}/.cutroom-{}",
                        current.rsplit_once('/').unwrap().0,
                        Uuid::new_v4()
                    );
                    j.relocate(&current, &temp)?;
                    j.relocate(&temp, new)?;
                } else {
                    j.relocate(&current, new)?;
                }
                moves.push((current, new.clone()));
            } else if migrate && !checked(root, new)?.exists() {
                j.mkdir(new)?;
            }
        } else {
            j.mkdir(new)?;
        }
    }
    for f in &mut p.folders {
        if migrate || before.folders.get(&f.id) != paths.get(&f.id) {
            f.disk_key = Some(scan::identity(&checked(root, &paths[&f.id])?)?);
        }
    }
    let mut reserved: HashSet<_> = p
        .assets
        .iter()
        .map(|a| translate(&a.project_relative_file(), &moves).to_lowercase())
        .collect();
    for a in &mut p.assets {
        let original = a.project_relative_file().into_owned();
        let old = translate(&original, &moves);
        let before = before.assets.get(&a.id);
        let relocated = old != original;
        let changed = before
            .is_none_or(|(f, n, h)| *f != a.folder_id || n != &a.name || *h != a.removed_from_bin);
        if !migrate && !changed && !relocated && a.project_file.is_some() {
            continue;
        }
        let parent = match a.folder_id {
            Some(id) => paths
                .get(&id)
                .ok_or("소스 폴더를 찾을 수 없습니다.")?
                .as_str(),
            None => "media",
        };
        let desired = if a.removed_from_bin {
            j.mkdir(ARCHIVE)?;
            format!("{ARCHIVE}/{}.{}", a.id, a.extension)
        } else if !migrate
            && a.project_file.is_some()
            && old.rsplit_once('/').is_some_and(|(d, _)| d == parent)
            && before.is_none_or(|(_, n, _)| n == &a.name)
        {
            old.clone()
        } else {
            unique(root, parent, a, &reserved, &old)?
        };
        if a.missing {
            a.project_file = Some(desired);
            continue;
        }
        let path = checked(root, &old)?;
        if old != desired && path.is_file() {
            if !migrate
                && a.disk
                    .as_ref()
                    .is_some_and(|d| scan::stamp(&path).is_ok_and(|now| now.key != d.key))
            {
                return Err(
                    "소스가 외부에서 변경되었습니다. 보관함을 새로 고친 뒤 다시 시도해 주세요."
                        .into(),
                );
            }
            if old.eq_ignore_ascii_case(&desired) {
                let temp = format!(
                    "{}/.cutroom-{}",
                    old.rsplit_once('/').unwrap().0,
                    Uuid::new_v4()
                );
                j.relocate(&old, &temp)?;
                j.relocate(&temp, &desired)?;
            } else {
                j.relocate(&old, &desired)?;
            }
        }
        a.project_file = Some(desired.clone());
        reserved.insert(desired.to_lowercase());
        let final_path = checked(root, &desired)?;
        a.missing = !final_path.is_file();
        if !a.missing {
            a.disk = Some(scan::stamp(&final_path)?);
        }
    }
    for (id, old) in &before.folders {
        if !paths.contains_key(id) {
            let dir = translate(old, &moves);
            let abs = checked(root, &dir)?;
            if abs.is_dir() {
                // Unregistered files also survive the editor's "move contents up" action.
                let parent = dir.rsplit_once('/').unwrap().0;
                for item in fs::read_dir(abs).map_err(|e| e.to_string())? {
                    let leaf = item
                        .map_err(|e| e.to_string())?
                        .file_name()
                        .to_string_lossy()
                        .into_owned();
                    j.relocate(&format!("{dir}/{leaf}"), &format!("{parent}/{leaf}"))?;
                }
                j.remove_empty(&dir)?;
            }
        }
    }
    p.media_tree = true;
    Ok(())
}

impl ProjectStore {
    pub(super) fn sync_media(&self) -> Result<(Project, bool), String> {
        self.reconcile_media(false)
    }

    pub(super) fn reconcile_media(
        &self,
        repair_durations: bool,
    ) -> Result<(Project, bool), String> {
        let _lock = self.lock_project()?;
        let mut p = self.load()?;
        let root = self.directory();
        recover(&root, p.media_transaction)?;
        let mut j = Journal::new(&root);
        let old_marker = p.media_transaction;
        let result: Result<bool, String> = (|| {
            let migrating = !p.media_tree;
            if migrating {
                let before = Layout::capture(&p)?;
                materialize(&root, &mut p, &before, &mut j, true)?;
            }
            let changed = scan::reconcile(self, &mut p, repair_durations)? || migrating;
            if changed {
                if let Some(marker) = j.marker() {
                    p.media_transaction = Some(marker);
                }
                super::save(&root, &p)?;
            }
            Ok(changed)
        })();
        j.finish(if result.is_ok() {
            p.media_transaction
        } else {
            old_marker
        })?;
        Ok((p, result?))
    }
}

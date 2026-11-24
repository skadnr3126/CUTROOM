//! Portable project data. Live locks, recovery journals and local disk identities stay on the host.
use super::{Project, ProjectStore, asset_filename::filename, media_tree};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
use tempfile::{NamedTempFile, TempDir};
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const HEADER: &str = "cutroom-package.json";
const ID_FILE: &str = ".cutroom-project-id";
#[cfg(test)]
mod tests;
#[derive(Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct Header {
    version: u32,
    pub id: Uuid,
    pub title: String,
    pub edited_ms: u64,
    #[serde(default)]
    source_ids: Vec<Uuid>,
}

impl Header {
    pub fn matches_older_copy(&self, store: &ProjectStore) -> bool {
        if self.source_ids.is_empty() || store.directory().join(ID_FILE).exists() {
            return false;
        }
        store.load().is_ok_and(|p| {
            p.title == self.title
                && self.source_ids.iter().all(|id| {
                    p.scenes.iter().any(|s| s.id == *id) || p.assets.iter().any(|a| a.id == *id)
                })
        })
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(super) fn edited_ms(dir: &Path) -> Result<u64, String> {
    let mut latest = UNIX_EPOCH;
    for file in ["project.json", "script-draft.json"] {
        match fs::metadata(dir.join(file)) {
            Ok(meta) => latest = latest.max(meta.modified().map_err(err)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(err(e)),
        }
    }
    Ok(latest
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64)
}

pub(super) fn identity(dir: &Path, local: Uuid) -> Result<Uuid, String> {
    match fs::read_to_string(dir.join(ID_FILE)) {
        Ok(s) => Uuid::parse_str(s.trim()).map_err(err),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(local),
        Err(e) => Err(err(e)),
    }
}

fn ignored(name: &str, root: bool) -> bool {
    let lower = name.to_ascii_lowercase();
    (root
        && matches!(
            lower.as_str(),
            "project.lock" | HEADER | ID_FILE | ".cutroom-media-transaction"
        ))
        || lower.ends_with(".cutroom.zip")
        || lower.starts_with(".drop-")
        || lower.starts_with(".capture-")
        || lower.starts_with(".tmp")
}

fn add_tree(zip: &mut ZipWriter<File>, dir: &Path, prefix: &str) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(err)? {
        let entry = entry.map_err(err)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "파일 이름을 읽을 수 없습니다.")?;
        if ignored(&name, prefix.is_empty()) || (prefix.is_empty() && name == "project.json") {
            continue;
        }
        if !media_tree::safe_component(&name) {
            return Err(format!("옮길 수 없는 파일 이름입니다. {name}"));
        }
        let rel = format!("{prefix}{name}");
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(err)?;
        if media_tree::linked(&meta) {
            return Err(format!(
                "연결된 파일은 원본을 프로젝트에 복사해 주세요. {rel}"
            ));
        }
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        if meta.is_dir() {
            let folder = format!("{rel}/");
            zip.add_directory(&folder, stored).map_err(err)?;
            add_tree(zip, &path, &folder)?;
        } else if meta.is_file() {
            let text = path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                matches!(
                    s.to_ascii_lowercase().as_str(),
                    "json" | "ass" | "srt" | "vtt" | "txt" | "md"
                )
            });
            let options = if text {
                stored
                    .compression_method(CompressionMethod::Deflated)
                    .compression_level(Some(1))
            } else {
                stored
            };
            zip.start_file(
                &rel,
                options.large_file(meta.len() >= u32::MAX as u64 - 65536),
            )
            .map_err(err)?;
            let mut file = File::open(&path).map_err(|e| format!("{rel}: {e}"))?;
            let bytes = io::copy(&mut file, zip).map_err(err)?;
            if bytes != meta.len()
                || file.metadata().map_err(err)?.modified().map_err(err)?
                    != meta.modified().map_err(err)?
            {
                return Err(format!(
                    "내보내는 중 파일이 변경되었습니다. 다시 시도해 주세요. {rel}"
                ));
            }
        }
    }
    Ok(())
}

fn check_assets(project: &Project, root: &Path) -> Result<(), String> {
    for a in &project.assets {
        let path = media_tree::checked(root, &a.project_relative_file())?;
        if !a.missing && !path.is_file() {
            return Err(format!("소스 파일을 찾을 수 없습니다. {}", a.name));
        }
    }
    Ok(())
}

pub(super) fn export(store: &ProjectStore, dest: &Path) -> Result<(), String> {
    let root = store.directory();
    let parent = dest.parent().ok_or("저장 폴더를 선택해 주세요.")?;
    if dunce::canonicalize(parent)
        .map_err(err)?
        .starts_with(dunce::canonicalize(&root).map_err(err)?)
    {
        return Err("패키지는 프로젝트 폴더 밖에 저장해 주세요.".into());
    }
    let _lock = store.lock_project()?;
    let mut project = store.load()?;
    media_tree::recover(&root, project.media_transaction)?;
    check_assets(&project, &root)?;
    let mut id = identity(&root, store.id)?;
    if id.is_nil() {
        id = Uuid::new_v4();
    }
    if !root.join(ID_FILE).try_exists().map_err(err)? {
        let mut temp = NamedTempFile::new_in(&root).map_err(err)?;
        write!(temp, "{id}").map_err(err)?;
        temp.persist_noclobber(root.join(ID_FILE)).map_err(err)?;
    }
    let header = Header {
        version: 1,
        id,
        title: project.title.clone(),
        edited_ms: edited_ms(&root)?,
        source_ids: project
            .scenes
            .iter()
            .take(4)
            .map(|s| s.id)
            .chain(project.assets.iter().take(4).map(|a| a.id))
            .collect(),
    };
    for a in &mut project.assets {
        a.disk = None;
    }
    for f in &mut project.folders {
        f.disk_key = None;
    }
    project.media_transaction = None;
    let tmp = NamedTempFile::new_in(parent).map_err(err)?;
    let mut zip = ZipWriter::new(tmp.reopen().map_err(err)?);
    let json = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(1));
    zip.start_file(HEADER, json).map_err(err)?;
    serde_json::to_writer(&mut zip, &header).map_err(err)?;
    zip.start_file("project.json", json).map_err(err)?;
    serde_json::to_writer(&mut zip, &project).map_err(err)?;
    add_tree(&mut zip, &root, "")?;
    zip.finish().map_err(err)?.sync_all().map_err(err)?;
    tmp.persist(dest).map_err(err)?;
    Ok(())
}

pub(super) struct Package {
    zip: ZipArchive<File>,
    pub header: Header,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(
    tag = "stage",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ImportProgress {
    Extracting {
        percent: u8,
        written_bytes: u64,
        total_bytes: u64,
    },
    Verifying,
    Opening,
}

impl Package {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut zip = ZipArchive::new(File::open(path).map_err(err)?).map_err(err)?;
        let header: Header = {
            let mut f = zip
                .by_name(HEADER)
                .map_err(|_| "컷룸 프로젝트 패키지를 선택해 주세요.")?;
            if f.size() > 64 * 1024 {
                return Err("패키지 정보가 너무 큽니다.".into());
            }
            let mut data = Vec::new();
            (&mut f)
                .take(64 * 1024 + 1)
                .read_to_end(&mut data)
                .map_err(err)?;
            serde_json::from_slice(&data).map_err(err)?
        };
        if header.version != 1 || header.id.is_nil() {
            return Err("지원하지 않는 컷룸 패키지입니다.".into());
        }
        Ok(Self { zip, header })
    }

    pub fn unpack(
        &mut self,
        parent: &Path,
        copy: bool,
        mut report: impl FnMut(ImportProgress),
    ) -> Result<TempDir, String> {
        let stage = tempfile::Builder::new()
            .prefix(".cutroom-import-")
            .tempdir_in(parent)
            .map_err(err)?;
        let mut total = 0u64;
        for i in 0..self.zip.len() {
            let entry = self.zip.by_index_raw(i).map_err(err)?;
            if !entry.is_dir() && entry.name() != HEADER {
                total = total
                    .checked_add(entry.size())
                    .ok_or("패키지 용량을 확인할 수 없습니다.")?;
            }
        }
        if total > fs2::available_space(parent).map_err(err)? {
            return Err("가져올 폴더의 여유 공간이 부족합니다.".into());
        }
        let mut buffer = vec![0u8; 256 * 1024];
        let mut written = 0u64;
        let mut last_percent = 0;
        report(ImportProgress::Extracting {
            percent: 0,
            written_bytes: 0,
            total_bytes: total,
        });
        let mut seen = HashSet::new();
        for i in 0..self.zip.len() {
            let mut file = self.zip.by_index(i).map_err(err)?;
            let name = file.name().trim_end_matches('/').to_owned();
            if !name.split('/').all(media_tree::safe_component)
                || file.is_symlink()
                || !seen.insert(name.to_lowercase())
            {
                return Err(format!("잘못되거나 중복된 패키지 경로입니다. {name}"));
            }
            if name == HEADER {
                continue;
            }
            if name
                .split('/')
                .enumerate()
                .any(|(i, part)| ignored(part, i == 0))
            {
                return Err(format!("프로젝트에 사용할 수 없는 파일입니다. {name}"));
            }
            let out = stage.path().join(&name);
            if file.is_dir() {
                fs::create_dir_all(&out).map_err(err)?;
                continue;
            }
            let size = file.size();
            fs::create_dir_all(out.parent().unwrap()).map_err(err)?;
            let mut dest = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(out)
                .map_err(err)?;
            let mut file_bytes = 0u64;
            loop {
                let n = match file.read(&mut buffer) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    result => result.map_err(err)?,
                };
                if n == 0 {
                    break;
                }
                if n as u64 > size - file_bytes {
                    return Err(format!("패키지 파일의 크기가 일치하지 않습니다. {name}"));
                }
                dest.write_all(&buffer[..n]).map_err(err)?;
                file_bytes += n as u64;
                written += n as u64;
                let percent = (u128::from(written) * 100 / u128::from(total)) as u8;
                if percent != last_percent {
                    report(ImportProgress::Extracting {
                        percent,
                        written_bytes: written,
                        total_bytes: total,
                    });
                    last_percent = percent;
                }
            }
            if file_bytes != size {
                return Err(format!("패키지 파일의 크기가 일치하지 않습니다. {name}"));
            }
        }
        report(ImportProgress::Verifying);
        let store = ProjectStore::at(parent.into(), stage.path().into(), Uuid::new_v4());
        let project = store.load()?;
        if project.title != self.header.title {
            return Err("패키지와 프로젝트의 제목이 일치하지 않습니다.".into());
        }
        check_assets(&project, stage.path())?;
        fs::create_dir_all(store.media_dir()).map_err(err)?;
        let id = if copy { Uuid::new_v4() } else { self.header.id };
        fs::write(stage.path().join(ID_FILE), id.to_string()).map_err(err)?;
        let modified = UNIX_EPOCH
            .checked_add(Duration::from_millis(self.header.edited_ms))
            .ok_or("잘못된 편집 시각입니다.")?;
        for name in ["project.json", "script-draft.json"] {
            let path = stage.path().join(name);
            if path.exists() {
                File::options()
                    .write(true)
                    .open(path)
                    .map_err(err)?
                    .set_modified(modified)
                    .map_err(err)?;
            }
        }
        Ok(stage)
    }
}

// Publish the validated tree in one rename on the destination volume.
pub(super) fn publish(stage: TempDir, parent: &Path, title: &str) -> Result<PathBuf, String> {
    for n in 1.. {
        let path = parent.join(filename(title, n));
        if path.try_exists().map_err(err)? {
            continue;
        }
        match fs::rename(stage.path(), &path) {
            Ok(()) => return Ok(path),
            Err(_) if path.exists() => continue,
            Err(e) => return Err(err(e)),
        }
    }
    unreachable!()
}

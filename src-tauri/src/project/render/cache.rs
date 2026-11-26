//! Reusable pre-BGM MP4 parts. Cache I/O is optional; staging paths belong to one export.
mod content;
mod fingerprint;
#[cfg(test)]
mod tests;

use super::{Project, SceneSettings, SceneStyle};
use content::copy_and_hash;
use fingerprint::{Dependencies, Inputs};
use std::{
    collections::HashMap,
    fs::{self, File, FileTimes},
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tempfile::{Builder, NamedTempFile};

const LIMIT: u64 = 4 * 1024 * 1024 * 1024;
const FREE_RESERVE: u64 = 1024 * 1024 * 1024;
const HEADER: u64 = 32; // SHA-256 of the following MP4 bytes, in the same atomic file.

pub(super) struct PartCache {
    root: PathBuf,
    inputs: Inputs,
    dependencies: HashMap<String, Dependencies>,
}

impl PartCache {
    pub(super) fn open(root: &Path, ffmpeg: &Path, staged_font: Option<&Path>) -> Option<Self> {
        fs::create_dir_all(root).ok()?;
        Some(Self {
            root: root.to_owned(),
            inputs: Inputs::new(ffmpeg, staged_font).ok()?,
            dependencies: HashMap::new(),
        })
    }

    pub(super) fn key(
        &mut self,
        project: &Project,
        media: &Path,
        piece: &SceneSettings,
        style: SceneStyle<'_>,
        ass: Option<(&str, u64)>,
        encoder_id: &str,
    ) -> Option<String> {
        let (key, sources) = self
            .inputs
            .key(project, media, piece, style, ass, encoder_id)
            .ok()?;
        self.dependencies.insert(key.clone(), sources);
        Some(key)
    }

    fn entry(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.part"))
    }

    pub(super) fn restore(&self, key: &str, local_part: &Path) -> bool {
        self.restore_file(key, local_part).is_ok()
    }

    fn restore_file(&self, key: &str, local_part: &Path) -> io::Result<()> {
        let sources = self.dependencies.get(key).ok_or(io::ErrorKind::NotFound)?;
        let mut entry = File::open(self.entry(key))?;
        let mut expected = [0; HEADER as usize];
        entry.read_exact(&mut expected)?;
        let mut temp =
            NamedTempFile::new_in(local_part.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
        let (digest, length) = copy_and_hash(&mut entry, temp.as_file_mut())?;
        if length == 0 || digest != expected || !sources.current() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        temp.persist(local_part)?;
        if let Ok(touch) = File::options().write(true).open(self.entry(key)) {
            let _ = touch.set_times(FileTimes::new().set_modified(SystemTime::now()));
        }
        Ok(())
    }

    pub(super) fn publish(&self, key: &str, local_part: &Path) {
        let _ = self.publish_file(key, local_part);
    }

    fn publish_file(&self, key: &str, local_part: &Path) -> io::Result<()> {
        let sources = self.dependencies.get(key).ok_or(io::ErrorKind::NotFound)?;
        let mut part = File::open(local_part)?;
        let length = part.metadata()?.len();
        if length == 0
            || length + HEADER > LIMIT
            || fs2::available_space(&self.root)? < length.saturating_add(FREE_RESERVE)
        {
            return Ok(());
        }
        let mut temp = Builder::new().prefix(".partial-").tempfile_in(&self.root)?;
        temp.write_all(&[0; HEADER as usize])?;
        let (digest, copied) = copy_and_hash(&mut part, temp.as_file_mut())?;
        if copied != length || !sources.current() {
            return Ok(());
        }
        temp.rewind()?;
        temp.write_all(&digest)?;
        temp.persist(self.entry(key))?;
        self.prune();
        Ok(())
    }

    pub(super) fn prune(&self) {
        let _ = prune(&self.root, LIMIT);
    }
}

fn prune(root: &Path, limit: u64) -> io::Result<()> {
    let mut entries = Vec::new();
    let mut bytes = 0u64;
    for entry in fs::read_dir(root)?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let age = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if name.starts_with(".partial-") {
            if age
                .elapsed()
                .is_ok_and(|elapsed| elapsed > Duration::from_secs(86_400))
            {
                let _ = fs::remove_file(entry.path());
            }
            continue;
        }
        if name
            .strip_suffix(".part")
            .is_some_and(|key| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            bytes = bytes.saturating_add(meta.len());
            entries.push((age, meta.len(), entry.path()));
        }
    }
    if bytes > limit {
        entries.sort_unstable();
        for (_, size, path) in entries {
            if fs::remove_file(path).is_ok() {
                bytes = bytes.saturating_sub(size);
            }
            if bytes <= limit {
                break;
            }
        }
    }
    Ok(())
}

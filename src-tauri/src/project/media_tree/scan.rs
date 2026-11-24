use super::*;
use crate::project::{files, folders::SourceFolder, model::Kind};
use std::{fs::File, time::UNIX_EPOCH};

#[cfg(windows)]
pub(super) fn identity(path: &Path) -> Result<String, String> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle,
        },
    };
    let handle = File::options()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(handle.as_raw_handle()), &mut info) }
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{:x}-{:x}-{:x}-{:x}-{:x}",
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
        info.ftCreationTime.dwHighDateTime,
        info.ftCreationTime.dwLowDateTime
    ))
}
#[cfg(unix)]
pub(super) fn identity(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path).map_err(|e| e.to_string())?;
    Ok(format!("{}-{}", m.dev(), m.ino()))
}
pub(super) fn stamp(path: &Path) -> Result<DiskStamp, String> {
    let m = fs::metadata(path).map_err(|e| e.to_string())?;
    Ok(DiskStamp {
        key: identity(path)?,
        size: m.len(),
        modified: m
            .modified()
            .map_err(|e| e.to_string())?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    })
}

struct Inventory {
    folders: Vec<(String, String)>,
    files: Vec<(String, DiskStamp)>,
}
fn walk(root: &Path, rel: &str, inventory: &mut Inventory) -> Result<(), String> {
    for entry in fs::read_dir(checked(root, rel)?).map_err(|e| e.to_string())? {
        let e = entry.map_err(|e| e.to_string())?;
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !safe_component(&name)
            || name.starts_with(".cutroom-")
            || name.starts_with(".drop-")
            || name.starts_with(".capture-")
            || name.starts_with(".tmp")
        {
            continue;
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err.to_string()),
        };
        if linked(&meta) {
            continue;
        }
        let child = format!("{rel}/{name}");
        if meta.is_dir() {
            inventory.folders.push((child.clone(), identity(&path)?));
            walk(root, &child, inventory)?;
        } else if meta.is_file()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .and_then(|s| Kind::from_extension(&s.to_ascii_lowercase()))
                .is_some()
        {
            inventory.files.push((child, stamp(&path)?));
        }
    }
    Ok(())
}

pub(super) fn reconcile(
    store: &ProjectStore,
    p: &mut Project,
    repair_durations: bool,
) -> Result<bool, String> {
    let mut inventory = Inventory {
        folders: vec![],
        files: vec![],
    };
    let root = store.directory();
    walk(&root, "media", &mut inventory)?;
    let prev = folder_paths(p)?;
    let mut changed = false;
    let mut folders = Vec::new();
    let mut folder_ids = HashMap::new();
    let dir_keys: HashSet<_> = inventory
        .folders
        .iter()
        .map(|(_, key)| key.clone())
        .collect();
    for (path, key) in inventory.folders {
        let (parent, name) = path.rsplit_once('/').unwrap();
        let parent_id = folder_ids.get(parent).copied();
        let old = p
            .folders
            .iter()
            .find(|f| f.disk_key.as_ref() == Some(&key))
            .or_else(|| {
                p.folders.iter().find(|f| {
                    prev.get(&f.id) == Some(&path)
                        && f.disk_key.as_ref().is_none_or(|k| !dir_keys.contains(k))
                })
            });
        let id = old.map_or_else(Uuid::new_v4, |f| f.id);
        changed |= old.is_none_or(|f| {
            f.name != name || f.parent_id != parent_id || f.disk_key.as_ref() != Some(&key)
        });
        folders.push(SourceFolder {
            id,
            name: name.into(),
            parent_id,
            disk_key: Some(key),
        });
        folder_ids.insert(path, id);
    }
    changed |= folders.len() != p.folders.len();
    p.folders = folders;
    let mut known_paths = HashMap::new();
    for (i, a) in p
        .assets
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.removed_from_bin)
    {
        let key = a.project_relative_file().to_lowercase();
        if a.missing {
            known_paths.entry(key).or_insert(i);
        } else {
            known_paths.insert(key, i);
        }
    }
    let mut old_keys = HashMap::<String, Vec<usize>>::new();
    for (i, a) in p
        .assets
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.removed_from_bin)
    {
        if let Some(d) = &a.disk {
            old_keys.entry(d.key.clone()).or_default().push(i);
        }
    }
    let mut counts = HashMap::new();
    for (_, d) in &inventory.files {
        *counts.entry(d.key.clone()).or_insert(0usize) += 1;
    }
    let mut found = HashSet::new();
    let probe = files::find_tool(&store.app_data, "ffprobe");
    for (rel, disk) in inventory.files {
        let (parent, leaf) = rel.rsplit_once('/').unwrap();
        let folder = folder_ids.get(parent).copied();
        let by_key = old_keys
            .get(&disk.key)
            .filter(|v| v.len() == 1 && counts[&disk.key] == 1)
            .map(|v| v[0]);
        let index = by_key.or_else(|| {
            known_paths.get(&rel.to_lowercase()).copied().filter(|i| {
                p.assets[*i]
                    .disk
                    .as_ref()
                    .is_none_or(|d| d.key == disk.key || !counts.contains_key(&d.key))
            })
        });
        if let Some(i) = index {
            let a = &mut p.assets[i];
            found.insert(a.id);
            if a.disk.as_ref() == Some(&disk)
                && a.project_file.as_ref() == Some(&rel)
                && a.folder_id == folder
                && !a.missing
            {
                if repair_durations
                    && a.kind != Kind::Image
                    && a.duration_ms.is_none()
                    && probe.is_some()
                {
                    if let Some(fresh) =
                        inspect_complete(&root.join(&rel), leaf, &disk, probe.as_deref())
                        && fresh.duration_ms.is_some()
                    {
                        a.duration_ms = fresh.duration_ms;
                        a.has_audio = fresh.has_audio;
                        changed = true;
                    }
                }
                continue;
            }
            let ext = Path::new(leaf)
                .extension()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            if a.disk.as_ref().is_some_and(|old| {
                old.modified != disk.modified || old.size != disk.size || old.key != disk.key
            }) || ext != a.extension
            {
                let Some(fresh) = inspect_complete(&root.join(&rel), leaf, &disk, probe.as_deref())
                else {
                    continue;
                };
                // A synced copy can acquire a new disk ID and millisecond timestamps.
                // Prefer measured metadata. An unavailable probe supplies no new evidence.
                let copied = a.project_file.as_deref() == Some(&rel)
                    && a.extension == ext
                    && a.disk.as_ref().is_some_and(|old| {
                        old.key != disk.key
                            && old.size == disk.size
                            && old.modified / 1_000_000 == disk.modified / 1_000_000
                    });
                a.extension = fresh.extension;
                a.kind = fresh.kind;
                if fresh.duration_ms.is_some() || !copied {
                    a.duration_ms = fresh.duration_ms;
                }
                if probe.is_some() || !copied {
                    a.has_audio = fresh.has_audio;
                }
            }
            if a.project_file.as_deref().and_then(|s| s.rsplit('/').next()) != Some(leaf) {
                a.name = leaf.into();
            }
            a.project_file = Some(rel);
            a.folder_id = folder;
            a.disk = Some(disk);
            a.missing = false;
            changed = true;
        } else {
            let Some(mut a) = inspect_complete(&root.join(&rel), leaf, &disk, probe.as_deref())
            else {
                continue;
            };
            a.folder_id = folder;
            a.project_file = Some(rel);
            a.disk = Some(disk);
            found.insert(a.id);
            p.assets.push(a);
            changed = true;
        }
    }
    for a in &mut p.assets {
        if a.removed_from_bin {
            continue;
        }
        if !found.contains(&a.id) {
            if !a.missing {
                a.missing = true;
                changed = true;
            }
            if a.folder_id
                .is_some_and(|id| !p.folders.iter().any(|f| f.id == id))
            {
                a.folder_id = None;
                changed = true;
            }
        }
    }
    Ok(changed)
}

fn inspect_complete(
    path: &Path,
    name: &str,
    observed: &DiskStamp,
    probe: Option<&Path>,
) -> Option<Asset> {
    #[cfg(windows)]
    let _guard = {
        use std::os::windows::fs::OpenOptionsExt;
        File::options().read(true).share_mode(1).open(path).ok()?
    };
    if observed.size == 0 {
        return None;
    }
    let a = files::inspect_asset(path, name, probe).ok()?;
    (stamp(path).ok().as_ref() == Some(observed)).then_some(a)
}

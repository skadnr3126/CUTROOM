use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

pub(super) fn copy_and_hash(
    input: &mut impl Read,
    output: &mut impl Write,
) -> io::Result<([u8; 32], u64)> {
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        output.write_all(&buffer[..n])?;
        count += n as u64;
    }
    Ok((hash.finalize().into(), count))
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub(super) struct Stamp {
    bytes: u64,
    modified: SystemTime,
    identity: (u64, u64, i64, i64),
}

pub(super) fn stamp(file: &File) -> io::Result<Stamp> {
    let meta = file.metadata()?;
    #[cfg(windows)]
    let identity = {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo,
                GetFileInformationByHandle, GetFileInformationByHandleEx,
            },
        };
        let handle = HANDLE(file.as_raw_handle());
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        let mut basic = FILE_BASIC_INFO::default();
        // Both queries borrow the still-open file and write exactly the supplied structure.
        unsafe {
            GetFileInformationByHandle(handle, &mut info)?;
            GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                (&mut basic as *mut FILE_BASIC_INFO).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            )?;
        }
        (
            info.dwVolumeSerialNumber as u64,
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            basic.ChangeTime,
            basic.CreationTime,
        )
    };
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
    };
    Ok(Stamp {
        bytes: meta.len(),
        modified: meta.modified()?,
        identity,
    })
}

pub(super) fn input_file(path: &Path) -> io::Result<File> {
    let mut options = File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ};
        // A writer may have modified mapped data without closing yet. Such inputs
        // still render normally; they are ineligible for caching during the write.
        options.share_mode((FILE_SHARE_READ | FILE_SHARE_DELETE).0);
    }
    options.open(path)
}

#[cfg(windows)]
fn can_memoize(path: &Path, file: &File, observed: &Stamp) -> bool {
    use std::{
        os::windows::io::AsRawHandle,
        path::{Component, Prefix},
    };
    use windows::{
        Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationByHandleW},
        },
        core::PCWSTR,
    };
    if observed.identity.1 == 0 || observed.identity.2 == 0 {
        return false;
    }
    let Ok(resolved) = std::fs::canonicalize(path) else {
        return false;
    };
    let Some(Component::Prefix(prefix)) = resolved.components().next() else {
        return false;
    };
    let letter = match prefix.kind() {
        Prefix::VerbatimDisk(letter) | Prefix::Disk(letter) => letter,
        _ => return false,
    };
    let root = [u16::from(letter), ':' as u16, '\\' as u16, 0];
    let mut filesystem = [0u16; 32];
    const DRIVE_FIXED: u32 = 3;
    // UNC, mapped network drives, FAT/exFAT and unknown volumes hash fresh.
    unsafe {
        GetDriveTypeW(PCWSTR(root.as_ptr())) == DRIVE_FIXED
            && GetVolumeInformationByHandleW(
                HANDLE(file.as_raw_handle()),
                None,
                None,
                None,
                None,
                Some(&mut filesystem),
            )
            .is_ok()
            && filesystem.starts_with(&[78, 84, 70, 83, 0]) // NTFS
    }
}

#[cfg(not(windows))]
fn can_memoize(_: &Path, _: &File, _: &Stamp) -> bool {
    // st_ctime alone doesn't establish its reliability on NFS/FUSE.
    false
}

struct KnownDigest {
    path: PathBuf,
    stamp: Stamp,
    digest: [u8; 32],
}

pub(super) fn content_hash(path: &Path) -> io::Result<(Stamp, [u8; 32])> {
    static KNOWN: OnceLock<Mutex<VecDeque<KnownDigest>>> = OnceLock::new();
    let mut file = input_file(path)?;
    let observed = stamp(&file)?;
    let memo = can_memoize(path, &file, &observed).then(|| KNOWN.get_or_init(Mutex::default));
    if let Some(memo) = memo
        && let Ok(entries) = memo.lock()
        && let Some(entry) = entries
            .iter()
            .rev()
            .find(|e| e.path == path && e.stamp == observed)
    {
        return Ok((observed, entry.digest));
    }
    let (digest, count) = copy_and_hash(&mut file, &mut io::sink())?;
    if count != observed.bytes || stamp(&file)? != observed {
        return Err(io::ErrorKind::Interrupted.into());
    }
    if let Some(memo) = memo
        && let Ok(mut entries) = memo.lock()
    {
        entries.retain(|e| e.path != path);
        if entries.len() >= 512 {
            entries.pop_front();
        }
        entries.push_back(KnownDigest {
            path: path.to_owned(),
            stamp: observed.clone(),
            digest,
        });
    }
    Ok((observed, digest))
}

mod delayed;
use super::ProjectStore;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};
use tauri::WebviewWindow;
use tempfile::NamedTempFile;
use uuid::Uuid;
use windows::Win32::{
    Foundation::{GlobalFree, HWND},
    System::{
        Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::{CF_HDROP, DROPEFFECT_COPY},
    },
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
        Shell::{DROPFILES, SHCreateDataObject, SHDoDragDrop},
    },
};

use super::asset_filename::filename;

// Media belongs to the project. Applications that open a dropped path may write to it
// or reopen it later, so give them a persistent, independent copy.
fn named_copy(source: &Path, directory: &Path, name: &str) -> io::Result<PathBuf> {
    let digest: String = Sha256::digest(name.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let directory = directory.join(digest);
    fs::create_dir_all(&directory)?;
    let target = directory.join(name);
    let original = fs::metadata(source)?;
    let timestamp = original.modified()?;
    if fs::metadata(&target)
        .is_ok_and(|m| m.len() == original.len() && m.modified().ok() == Some(timestamp))
    {
        return Ok(target);
    }
    let temp = NamedTempFile::new_in(&directory)?;
    io::copy(&mut fs::File::open(source)?, &mut temp.as_file())?;
    temp.as_file().set_modified(timestamp)?;
    temp.persist(&target)?;
    Ok(target)
}

fn display_file_name(asset: &super::model::Asset, n: usize) -> String {
    if asset.project_file.is_some() {
        super::generated::named(&asset.name, &asset.extension, n)
    } else {
        filename(&asset.name, n)
    }
}

fn prepare(store: &ProjectStore, ids: Vec<Uuid>) -> Result<Vec<PathBuf>, String> {
    let project = store.load()?;
    let mut seen = HashSet::new();
    let mut assets = Vec::new();
    for id in ids {
        if seen.insert(id) {
            let asset = project.asset(id)?;
            if asset.removed_from_bin {
                return Err("보관함에서 삭제된 소스입니다.".into());
            }
            assets.push((asset, display_file_name(asset, 1)));
        }
    }
    if assets.is_empty() {
        return Err("복사할 소스를 선택해 주세요.".into());
    }
    let reserved: HashSet<_> = assets.iter().map(|(_, name)| name.to_lowercase()).collect();
    let mut used = HashSet::new();
    let mut paths = Vec::new();
    for (asset, mut name) in assets {
        if !used.insert(name.to_lowercase()) {
            for n in 2.. {
                name = display_file_name(asset, n);
                let key = name.to_lowercase();
                if !reserved.contains(&key) && used.insert(key) {
                    break;
                }
            }
        }
        let dir = store.app_data.join("drag-files").join(asset.id.to_string());
        paths.push(
            named_copy(&asset.path_in(&store.media_dir()), &dir, &name).map_err(|e| {
                format!("‘{}’ 파일을 복사할 준비를 하지 못했습니다. {e}", asset.name)
            })?,
        );
    }
    Ok(paths)
}

fn file_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
        ..Default::default()
    }
}

fn file_object(paths: &[PathBuf]) -> windows::core::Result<IDataObject> {
    use std::os::windows::ffi::OsStrExt;
    let mut list = Vec::<u16>::new();
    for path in paths {
        list.extend(path.as_os_str().encode_wide());
        list.push(0);
    }
    list.push(0);
    let offset = size_of::<DROPFILES>();
    unsafe {
        let data: IDataObject = SHCreateDataObject(None, None, None)?;
        let block = GlobalAlloc(GMEM_MOVEABLE, offset + list.len() * size_of::<u16>())?;
        let bytes = GlobalLock(block).cast::<u8>();
        if bytes.is_null() {
            let error = windows::core::Error::from_thread();
            let _ = GlobalFree(Some(block));
            return Err(error);
        }
        bytes.cast::<DROPFILES>().write_unaligned(DROPFILES {
            pFiles: offset as u32,
            fWide: true.into(),
            ..Default::default()
        });
        std::ptr::copy_nonoverlapping(list.as_ptr(), bytes.add(offset).cast::<u16>(), list.len());
        let _ = GlobalUnlock(block);
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: block },
            ..Default::default()
        };
        // SetData takes ownership of the HGLOBAL on success.
        if let Err(error) = data.SetData(&file_format(), &medium, true) {
            let _ = GlobalFree(Some(block));
            return Err(error);
        }
        Ok(data)
    }
}

fn begin(hwnd: HWND, store: ProjectStore, ids: Vec<Uuid>) -> Result<bool, String> {
    unsafe {
        if GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 || GetAsyncKeyState(VK_RBUTTON.0 as i32) < 0 {
            let (data, failure) = delayed::data_object(store, ids).map_err(|e| e.to_string())?;
            let result = SHDoDragDrop(Some(hwnd), &data, None, DROPEFFECT_COPY);
            if let Some(error) = failure.get() {
                return Err(error.clone());
            }
            return result
                .map(|effect| effect == DROPEFFECT_COPY)
                .map_err(|e| format!("파일 드래그를 시작할 수 없습니다. {e}"));
        }
    }
    Ok(false)
}

#[tauri::command]
pub(crate) async fn drag_bin_files(
    window: WebviewWindow,
    store: ProjectStore,
    ids: Vec<Uuid>,
) -> Result<bool, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let owner = window.clone();
    window
        .run_on_main_thread(move || {
            let result = owner
                .hwnd()
                .map_err(|e| e.to_string())
                .and_then(|hwnd| begin(hwnd, store, ids));
            let _ = tx.try_send(result);
        })
        .map_err(|e| e.to_string())?;
    rx.recv().await.ok_or("파일 드래그가 중단되었습니다.")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use windows::Win32::{
        System::Ole::{OleInitialize, OleUninitialize, ReleaseStgMedium},
        UI::Shell::{DragQueryFileW, HDROP},
    };

    fn fixture(root: &Path) -> (ProjectStore, [Uuid; 3]) {
        let store = ProjectStore::new(root.into());
        fs::create_dir_all(store.media_dir()).unwrap();
        let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let names = ["새 장면.png", "새 장면.png", "새 장면 (2).png"];
        let mut assets = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            fs::write(store.media_dir().join(format!("{id}.png")), [index as u8]).unwrap();
            assets.push(json!({"id":id,"name":names[index],"extension":"png","kind":"image","hasAudio":false}));
        }
        fs::write(
            store.directory().join("project.json"),
            json!({"title":"test","scenes":[],"assets":assets}).to_string(),
        )
        .unwrap();
        (store, ids)
    }

    #[test]
    fn windows_drop_contains_selected_files_with_library_names_and_independent_bytes() {
        let root = tempfile::tempdir().unwrap();
        let (store, ids) = fixture(root.path());
        let selection = vec![ids[0], ids[1], ids[1], ids[2]];
        assert!(prepare(&store, vec![Uuid::new_v4()]).is_err());
        unsafe {
            OleInitialize(None).unwrap();
            let (data, failure) = delayed::data_object(store.clone(), selection.clone()).unwrap();
            data.QueryGetData(&file_format()).ok().unwrap();
            assert!(!store.app_data.join("drag-files").exists());
            let mut medium = data.GetData(&file_format()).unwrap();
            assert!(failure.get().is_none());
            let paths = prepare(&store, selection.clone()).unwrap();
            assert_eq!(paths, prepare(&store, selection).unwrap());
            let drop = HDROP(medium.u.hGlobal.0);
            assert_eq!(DragQueryFileW(drop, u32::MAX, None), 3);
            for (i, expected) in ["새 장면.png", "새 장면 (3).png", "새 장면 (2).png"]
                .iter()
                .enumerate()
            {
                let n = DragQueryFileW(drop, i as u32, None);
                let mut buffer = vec![0; n as usize + 1];
                assert_eq!(DragQueryFileW(drop, i as u32, Some(&mut buffer)), n);
                let path = PathBuf::from(String::from_utf16(&buffer[..n as usize]).unwrap());
                assert_eq!(path.file_name().unwrap(), *expected);
                assert_eq!(fs::read(&path).unwrap(), [i as u8]);
                fs::write(path, b"externally edited").unwrap();
                assert_eq!(
                    fs::read(store.media_dir().join(format!("{}.png", ids[i]))).unwrap(),
                    [i as u8]
                );
            }
            ReleaseStgMedium(&mut medium);
            std::mem::drop(data);
            OleUninitialize();
        }
    }

    #[test]
    fn explorer_folder_accepts_the_batch_as_a_copy() {
        use windows::{
            Win32::{
                System::{Ole::IDropTarget, SystemServices::MK_LBUTTON},
                UI::Shell::{BHID_SFUIObject, IShellItem, SHCreateItemFromParsingName},
            },
            core::HSTRING,
        };
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("destination");
        fs::create_dir(&dest).unwrap();
        let (store, ids) = fixture(dir.path());
        unsafe {
            OleInitialize(None).unwrap();
            {
                let folder: IShellItem =
                    SHCreateItemFromParsingName(&HSTRING::from(dest.as_os_str()), None).unwrap();
                let target: IDropTarget = folder.BindToHandler(None, &BHID_SFUIObject).unwrap();
                let (data, _) = delayed::data_object(store.clone(), ids.to_vec()).unwrap();
                let mut effect = DROPEFFECT_COPY;
                target
                    .DragEnter(&data, MK_LBUTTON, Default::default(), &mut effect)
                    .unwrap();
                assert_eq!(effect, DROPEFFECT_COPY);
                target
                    .Drop(&data, Default::default(), Default::default(), &mut effect)
                    .unwrap();
                for (index, name) in ["새 장면.png", "새 장면 (3).png", "새 장면 (2).png"]
                    .iter()
                    .enumerate()
                {
                    assert_eq!(fs::read(dest.join(name)).unwrap(), [index as u8]);
                    assert!(
                        store
                            .media_dir()
                            .join(format!("{}.png", ids[index]))
                            .exists()
                    );
                }
            }
            OleUninitialize();
        }
    }

    #[test]
    fn display_names_are_safe_windows_path_components() {
        assert_eq!(filename("내 영상", 1), "내 영상");
        assert_eq!(filename("CON.txt", 1), "_CON.txt");
        assert_eq!(filename("../a:b?.png", 1), ".._a_b_.png");
        assert_eq!(filename(" .. ", 1), "_");
        assert!(
            filename(&format!("{}.mp4", "🎞".repeat(256)), 2)
                .encode_utf16()
                .count()
                <= 255
        );
    }
}

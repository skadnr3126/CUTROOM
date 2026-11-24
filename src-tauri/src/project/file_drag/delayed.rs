use super::{ProjectStore, file_format, file_object, prepare};
use std::{cell::OnceCell, rc::Rc};
use uuid::Uuid;
use windows::{
    Win32::{
        Foundation::{
            DV_E_FORMATETC, E_FAIL, E_NOTIMPL, E_POINTER, OLE_E_ADVISENOTSUPPORTED, S_OK,
        },
        System::Com::{
            DATADIR_GET, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC,
            IEnumSTATDATA, STGMEDIUM,
        },
        UI::Shell::{SHCreateDataObject, SHCreateStdEnumFmtEtc},
    },
    core::{BOOL, HRESULT, Ref, Result, implement},
};

// Start the OS drag immediately. A recipient can query supported formats without
// touching the media. Only a request for the file list materializes the named copies.
#[implement(IDataObject, Agile = false)]
struct Files {
    store: ProjectStore,
    ids: Vec<Uuid>,
    ready: OnceCell<Result<IDataObject>>,
    failure: Rc<OnceCell<String>>,
}

pub(super) fn data_object(
    store: ProjectStore,
    ids: Vec<Uuid>,
) -> Result<(IDataObject, Rc<OnceCell<String>>)> {
    let failure = Rc::new(OnceCell::new());
    let files: IDataObject = Files {
        store,
        ids,
        ready: OnceCell::new(),
        failure: failure.clone(),
    }
    .into();
    // The shell outer object owns private formats used for drag images and drop effects.
    Ok((unsafe { SHCreateDataObject(None, None, &files)? }, failure))
}

impl IDataObject_Impl for Files_Impl {
    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        let Some(format) = (unsafe { format.as_ref() }) else {
            return E_POINTER;
        };
        let expected = file_format();
        if format.cfFormat == expected.cfFormat
            && format.dwAspect == expected.dwAspect
            && format.lindex == -1
            && format.tymed & expected.tymed != 0
        {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetData(&self, format: *const FORMATETC) -> Result<STGMEDIUM> {
        self.QueryGetData(format).ok()?;
        let data = self
            .ready
            .get_or_init(|| {
                let paths = prepare(&self.store, self.ids.clone()).map_err(|message| {
                    let _ = self.failure.set(message);
                    windows::core::Error::from(E_FAIL)
                })?;
                file_object(&paths)
            })
            .as_ref()
            .map_err(Clone::clone)?;
        unsafe { data.GetData(format) }
    }

    fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        unsafe { SHCreateStdEnumFmtEtc(&[file_format()]) }
    }

    fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, _: *mut FORMATETC) -> HRESULT {
        E_NOTIMPL
    }
    fn SetData(&self, _: *const FORMATETC, _: *const STGMEDIUM, _: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn DAdvise(&self, _: *const FORMATETC, _: u32, _: Ref<IAdviseSink>) -> Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
    fn DUnadvise(&self, _: u32) -> Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

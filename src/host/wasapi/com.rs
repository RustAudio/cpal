//! Handles COM initialization and cleanup.

pub(super) mod audio;
pub(super) mod threading;

use super::IoError;
use std::fmt;
use std::marker::PhantomData;

use windows_sys::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

thread_local!(static COM_INITIALIZED: ComInitialized = {
    unsafe {
        // Try to initialize COM with STA by default to avoid compatibility issues with the ASIO
        // backend (where CoInitialize() is called by the ASIO SDK) or winit (where drag and drop
        // requires STA).
        // This call can fail with RPC_E_CHANGED_MODE if another library initialized COM with MTA.
        // That's OK though since COM ensures thread-safety/compatibility through marshalling when
        // necessary.
        let result = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED);
        if result >= 0 || result == RPC_E_CHANGED_MODE {
            ComInitialized {
                result,
                _ptr: PhantomData,
            }
        } else {
            // COM initialization failed in another way, something is really wrong.
            panic!("Failed to initialize COM: {}", IoError::from_raw_os_error(result));
        }
    }
});

/// RAII object that guards the fact that COM is initialized.
///
// We store a raw pointer because it's the only way at the moment to remove `Send`/`Sync` from the
// object.
struct ComInitialized {
    result: HRESULT,
    _ptr: PhantomData<*mut ()>,
}

impl Drop for ComInitialized {
    #[inline]
    fn drop(&mut self) {
        // Need to avoid calling CoUninitialize() if CoInitializeEx failed since it may have
        // returned RPC_E_MODE_CHANGED - which is OK, see above.
        if self.result >= 0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// Ensures that COM is initialized in this thread.
#[inline]
pub fn com_initialized() {
    COM_INITIALIZED.with(|_| {});
}

use std::ffi::c_void;
use windows_sys::core::{GUID, HRESULT};

pub(super) type ComResult<T> = Result<T, HRESULT>;

#[repr(C)]
pub(crate) struct Interface<T> {
    vtable: *mut T,
}

impl<T> Interface<T> {
    #[inline]
    pub(crate) fn vtbl(&self) -> &T {
        unsafe { &*self.vtable }
    }
}

#[repr(transparent)]
pub(super) struct Object<T>(*mut Interface<T>);

impl<T> Object<T> {
    #[inline]
    pub(crate) fn vtbl(&self) -> &T {
        unsafe { (*self.0).vtbl() }
    }

    #[inline]
    pub(crate) fn ptr(&self) -> *mut c_void {
        self.0.cast()
    }
}

impl<T> fmt::Debug for Object<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:p}", self.0)
    }
}

impl<T> Drop for Object<T> {
    fn drop(&mut self) {
        unsafe { ((*self.0.cast::<Interface<IUnknownV>>()).vtbl().release)(self.ptr()) };
    }
}

impl<T> Clone for Object<T> {
    fn clone(&self) -> Self {
        unsafe { ((*self.0.cast::<Interface<IUnknownV>>()).vtbl().add_ref)(self.ptr()) };
        Self(self.0)
    }
}

#[inline]
unsafe fn query_interface<I>(ptr: *mut c_void, iid: &GUID) -> ComResult<I> {
    let mut out = std::mem::MaybeUninit::<I>::uninit();
    let res = unsafe {
        ((*ptr.cast::<IUnknown>()).vtbl().query_interface)(ptr, iid, out.as_mut_ptr().cast())
    };
    if res >= 0 {
        Ok(unsafe { out.assume_init() })
    } else {
        Err(res)
    }
}

#[repr(C)]
pub(crate) struct IUnknownV {
    pub(crate) query_interface: unsafe extern "system" fn(
        this: *mut c_void,
        iid: *const GUID,
        interface: *mut *const c_void,
    ) -> HRESULT,
    pub(crate) add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub(crate) release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

type IUnknown = Interface<IUnknownV>;

use windows_sys::core::BSTR;

/// Length prefixed string
#[repr(transparent)]
pub(crate) struct Bstr(*const u16);

impl From<Bstr> for String {
    fn from(b: Bstr) -> Self {
        let len = unsafe { windows_sys::Win32::Foundation::SysStringLen(b.0) };
        if len == 0 {
            return String::new();
        }

        let s = unsafe { std::slice::from_raw_parts(b.0, len as usize) };
        let mut s = String::from_utf16_lossy(s);
        let trunc = s.trim_end().len();
        s.truncate(trunc);
        s
    }
}

impl Drop for Bstr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { windows_sys::Win32::Foundation::SysFreeString(self.0) }
        }
    }
}

#[repr(C)]
pub(crate) struct IErrorInfoV {
    base: IUnknownV,
    __get_guid: usize,
    __get_source: usize,
    pub(crate) get_description:
        unsafe extern "system" fn(this: *mut c_void, description: *mut BSTR) -> HRESULT,
    __get_help_file: usize,
    __get_help_context: usize,
}

#[repr(transparent)]
struct IErrorInfo(Object<IErrorInfoV>);

const IID_RESTRICTED_ERROR_INFO: GUID = GUID::from_u128(0x82ba7092_4c88_427d_a7bc_16dd93feb67e);

#[repr(C)]
pub(crate) struct IRestrictedErrorInfoV {
    base: IUnknownV,
    pub(crate) get_error_details: unsafe extern "system" fn(
        this: *mut c_void,
        description: *mut BSTR,
        error: *mut HRESULT,
        restricteddescription: *mut BSTR,
        capabilitysid: *mut BSTR,
    ) -> HRESULT,
    __get_reference: usize,
}

#[repr(transparent)]
struct IRestrictedErrorInfo(Object<IRestrictedErrorInfoV>);

#[repr(C)]
pub struct ILanguageExceptionErrorInfoV {
    base: IUnknownV,
    __get_language_exception: usize,
}

const IID_LANGUAGE_EXCEPTION_ERROR_INFO2: GUID =
    GUID::from_u128(0x5746e5c4_5b97_424c_b620_2822915734dd);

#[repr(C)]
struct ILanguageExceptionErrorInfo2V {
    base: ILanguageExceptionErrorInfoV,
    __get_previous: usize,
    capture_propagation_context:
        unsafe extern "system" fn(this: *mut c_void, languageexception: *mut c_void) -> HRESULT,
    __get_propagation_context_head: usize,
}

#[repr(transparent)]
struct ILanguageExceptionErrorInfo2(Object<ILanguageExceptionErrorInfo2V>);

pub(crate) fn get_error_message(e: HRESULT) -> String {
    use windows_sys::Win32::{
        Foundation::S_OK,
        System::{
            Com::GetErrorInfo,
            Diagnostics::Debug::{
                FormatMessageW, FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_FROM_SYSTEM,
                FORMAT_MESSAGE_IGNORE_INSERTS,
            },
        },
    };

    unsafe {
        // Check if the an error has been set, this is preferable to the message
        // for the HRESULT itself if it is available
        let mut error_info = std::mem::MaybeUninit::<IErrorInfo>::uninit();
        if GetErrorInfo(0, error_info.as_mut_ptr().cast()) == S_OK {
            let error_info = error_info.assume_init();

            // Check if we are on a newer version of windows and get access
            // to more detailed breadcrumb info
            if let Ok(restricted) = query_interface::<IRestrictedErrorInfo>(
                error_info.0.ptr(),
                &IID_RESTRICTED_ERROR_INFO,
            ) {
                if let Ok(cap) = query_interface::<ILanguageExceptionErrorInfo2>(
                    error_info.0.ptr(),
                    &IID_LANGUAGE_EXCEPTION_ERROR_INFO2,
                ) {
                    (cap.0.vtbl().capture_propagation_context)(
                        cap.0.ptr().cast(),
                        std::ptr::null_mut(),
                    );
                }

                let mut fallback = std::mem::MaybeUninit::uninit();
                let mut code = 0;
                let mut msg = std::mem::MaybeUninit::uninit();
                (restricted.0.vtbl().get_error_details)(
                    restricted.0.ptr(),
                    fallback.as_mut_ptr(),
                    &mut code,
                    msg.as_mut_ptr(),
                    std::ptr::null_mut(),
                );

                if e == code {
                    let mut msg: String = Bstr(msg.assume_init()).into();
                    if msg.is_empty() {
                        msg = Bstr(fallback.assume_init()).into();
                    }

                    return msg;
                }
            }

            let mut bs = std::mem::MaybeUninit::<Bstr>::uninit();
            if (error_info.0.vtbl().get_description)(error_info.0.ptr(), bs.as_mut_ptr().cast())
                == S_OK
            {
                let bs = bs.assume_init();
                bs.into()
            } else {
                String::new()
            }
        } else {
            let buffer: *mut u16 = std::ptr::null_mut();
            let size = FormatMessageW(
                FORMAT_MESSAGE_ALLOCATE_BUFFER
                    | FORMAT_MESSAGE_FROM_SYSTEM
                    | FORMAT_MESSAGE_IGNORE_INSERTS,
                std::ptr::null(),
                e as _,
                0,
                buffer,
                0,
                std::ptr::null(),
            );

            if size == 0 || buffer.is_null() {
                format!("{e}")
            } else {
                let utfs = std::slice::from_raw_parts(buffer as *const _, size as usize);

                let s = String::from_utf16_lossy(utfs);
                let trimmed = s.trim_end();
                let s = format!("{trimmed}{e}");

                windows_targets::link!("kernel32.dll" "system" fn LocalFree(hmem: *mut std::ffi::c_void) -> *mut std::ffi::c_void);

                LocalFree(buffer.cast());

                s
            }
        }
    }
}

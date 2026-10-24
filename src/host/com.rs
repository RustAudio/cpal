//! Handles COM initialization and cleanup.

use std::{io::Error as IoError, marker::PhantomData};

use windows::Win32::{
    Foundation::RPC_E_CHANGED_MODE,
    System::Com::{
        COINIT, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree,
        CoUninitialize,
    },
};

thread_local!(static COM_INITIALIZED: ComInitialized = {
    ComInitialized::new(COINIT_APARTMENTTHREADED)
});

thread_local!(static COM_INITIALIZED_MTA: ComInitialized = {
    ComInitialized::new(COINIT_MULTITHREADED)
});

/// RAII object that guards the fact that COM is initialized.
///
// We store a raw pointer because it's the only way at the moment to remove `Send`/`Sync` from the
// object.
struct ComInitialized {
    result: windows::core::HRESULT,
    _ptr: PhantomData<*mut ()>,
}

impl Drop for ComInitialized {
    fn drop(&mut self) {
        // Need to avoid calling CoUninitialize() if CoInitializeEx failed since it may have
        // returned RPC_E_MODE_CHANGED - which is OK, see above.
        if self.result.is_ok() {
            unsafe { CoUninitialize() };
        }
    }
}

impl ComInitialized {
    fn new(concurrency_model: COINIT) -> Self {
        unsafe {
            // This can fail with RPC_E_CHANGED_MODE if another library initialized COM with a
            // different model. That's OK: COM is already initialized on this thread.
            let result = CoInitializeEx(None, concurrency_model);
            if result.is_ok() || result == RPC_E_CHANGED_MODE {
                ComInitialized {
                    result,
                    _ptr: PhantomData,
                }
            } else {
                // COM initialization failed in another way, something is really wrong.
                panic!(
                    "Failed to initialize COM: {}",
                    IoError::from_raw_os_error(result.0)
                );
            }
        }
    }
}

/// RAII wrapper for COM-allocated wide strings freed with `CoTaskMemFree`.
pub(super) struct ComString(pub windows::core::PWSTR);

impl Drop for ComString {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.as_ptr() as *mut _)) }
    }
}

/// Ensures that COM is initialized in this thread.
pub fn com_initialized() {
    // Try to initialize COM as an STA by default to avoid compatibility issues with the ASIO
    // backend or UI frameworks such as winit. A thread already initialized as MTA keeps that
    // model when CoInitializeEx returns RPC_E_CHANGED_MODE.
    COM_INITIALIZED.with(|_| {});
}

/// Ensures COM is initialized with the multithreaded apartment on this thread.
///
/// Use this for system-managed worker threads so they do not become STAs, which are incompatible
/// with Windows thread pools.
pub fn com_initialized_mta() {
    COM_INITIALIZED_MTA.with(|_| {});
}

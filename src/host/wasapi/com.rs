//! Handles COM initialization and cleanup.

use std::ptr;

use super::winapi::um::objbase::{COINIT_MULTITHREADED};
use super::winapi::um::combaseapi::{CoInitializeEx, CoUninitialize};
use super::winapi::shared::winerror::{SUCCEEDED, HRESULT, RPC_E_CHANGED_MODE};

thread_local!(static COM_INITIALIZED: ComInitialized = {
    unsafe {
        // this call can fail with RPC_E_CHANGED_MODE if another library initialized COM
        // in apartment-threaded mode. That's OK though, we don't care.
        let result = CoInitializeEx(ptr::null_mut(), COINIT_MULTITHREADED);
        if SUCCEEDED(result) || result == RPC_E_CHANGED_MODE {
            ComInitialized {
                result
            }
        } else {
            // COM initialization failed in another way, something is really wrong.
            panic!("Failed to initialize COM.");
        }
    }
});

/// RAII object that guards the fact that COM is initialized.
struct ComInitialized {
    result: HRESULT,
}

impl Drop for ComInitialized {
    #[inline]
    fn drop(&mut self) {
        // Need to avoid calling CoUninitialize() if CoInitializeEx failed since it may have returned
        // RPC_E_MODE_CHANGED - which is OK, see above.
        if SUCCEEDED(self.result) {
            unsafe { CoUninitialize() };
        }
    }
}

/// Ensures that COM is initialized in this thread.
#[inline]
pub fn com_initialized() {
    COM_INITIALIZED.with(|_| {});
}

//! Handles COM initialization and cleanup.

use super::check_result;
use super::ole32;
use super::winapi;
use std::ptr;

thread_local!(static COM_INITIALIZED: ComInitialized = {
    unsafe {
        // this call can fail if another library initialized COM in single-threaded mode
        // handling this situation properly would make the API more annoying, so we just don't care
        check_result(ole32::CoInitializeEx(ptr::null_mut(), winapi::COINIT_MULTITHREADED)).unwrap();
        ComInitialized(ptr::null_mut())
    }
});

/// RAII object that guards the fact that COM is initialized.
///
// We store a raw pointer because it's the only way at the moment to remove `Send`/`Sync` from the
// object.
struct ComInitialized(*mut ());

impl Drop for ComInitialized {
    #[inline]
    fn drop(&mut self) {
        unsafe { ole32::CoUninitialize() };
    }
}

/// Ensures that COM is initialized in this thread.
#[inline]
pub fn com_initialized() {
    COM_INITIALIZED.with(|_| {});
}

//! macOS system audio recording permission helpers.
//!
//! These functions check and request the "System Audio Recording" permission
//! (`kTCCServiceAudioCapture`) via the private TCC framework — required for
//! loopback recording via [`default_output_device`](super::enumerate::default_output_device).

use block2::StackBlock;
use libloading::{Library, Symbol};
use objc2_core_foundation::{CFRetained, CFString};
use std::ffi::c_void;

const TCC_FRAMEWORK: &str = "/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC";
const TCC_SERVICE: &str = "kTCCServiceAudioCapture";

fn load_tcc() -> Option<Library> {
    unsafe { Library::new(TCC_FRAMEWORK) }.ok()
}

fn tcc_service() -> CFRetained<CFString> {
    CFString::from_str(TCC_SERVICE)
}

/// Request system audio recording permission, showing the system prompt if needed.
///
/// **Blocking** — does not return until the user responds.
/// Returns `false` immediately (without showing UI) if previously denied —
/// call [`open_system_audio_settings`] in that case.
pub fn request_system_audio_permission() -> bool {
    let Some(lib) = load_tcc() else { return false };
    unsafe {
        let Ok(request_fn): Result<
            Symbol<unsafe extern "C" fn(*const c_void, *const c_void, *const c_void)>,
            _,
        > = lib.get(b"TCCAccessRequest\0") else {
            return false;
        };

        let (tx, rx) = std::sync::mpsc::sync_channel::<bool>(1);
        // Store as usize (Copy) so TCC's internal block memcpy doesn't double-drop the sender.
        let tx_ptr = Box::into_raw(Box::new(tx)) as usize;

        let completion = StackBlock::new(move |granted: u8| {
            let tx = Box::from_raw(tx_ptr as *mut std::sync::mpsc::SyncSender<bool>);
            tx.send(granted != 0).ok();
        });

        let service = tcc_service();
        request_fn(
            &*service as *const _ as *const c_void,
            std::ptr::null(),
            &completion as *const _ as *const c_void,
        );

        rx.recv().unwrap_or(false)
    }
}

/// Open Privacy & Security > System Audio Recording in System Settings.
///
/// Call this when [`request_system_audio_permission`] returns `false` (previously denied).
pub fn open_system_audio_settings() {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AudioCapture")
        .spawn()
        .ok();
}

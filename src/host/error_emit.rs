//! Error delivery helpers for audio callbacks.
//!
//! Pick the helper based on what you need:
//! - Must not block (RT process callback): [`try_emit_error`]
//! - Caller must not miss the error: [`emit_error`] (blocks until callback available)
//! - Informational only, OK to drop if callback is busy: [`emit_error_or_warn`]
//!
//! Use bare `log::warn!` instead of these helpers when the user callback must not be invoked at
//! all (e.g., during stream construction before the `Stream` handle has been returned to the
//! caller). Note that `log::warn!` may allocate, so it is still not safe on an RT thread.

use crate::Error;
use std::sync::{Mutex, TryLockError};

/// Deliver an error, blocking until the callback is available.
pub(crate) fn emit_error<E>(callback: &Mutex<E>, error: Error)
where
    E: FnMut(Error) + Send + ?Sized,
{
    let mut cb = callback.lock().unwrap_or_else(|e| e.into_inner());
    (*cb)(error);
}

/// Try to deliver an error without blocking.
///
/// Returns `Ok(())` if the callback was invoked, or `Err(error)` if the lock was contended and
/// the error could not be delivered.
pub(crate) fn try_emit_error<E>(callback: &Mutex<E>, error: Error) -> Result<(), Error>
where
    E: FnMut(Error) + Send + ?Sized,
{
    match callback.try_lock() {
        Ok(mut cb) => {
            (*cb)(error);
            Ok(())
        }
        Err(TryLockError::Poisoned(e)) => {
            (*e.into_inner())(error);
            Ok(())
        }
        Err(TryLockError::WouldBlock) => Err(error),
    }
}

/// Try to deliver an error; log and discard it if the callback is busy.
#[cfg(all(
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
    ),
    feature = "pipewire"
))]
pub(crate) fn emit_error_or_warn<E>(callback: &Mutex<E>, error: Error)
where
    E: FnMut(Error) + Send + ?Sized,
{
    if let Err(_e) = try_emit_error(callback, error) {
        #[cfg(feature = "log")]
        log::warn!("cpal: {} (error callback busy; notification dropped)", _e);
    }
}

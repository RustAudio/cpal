//! Error delivery helpers for audio callbacks.
//!
//! Pick the helper based on what you need:
//! - Must not block (RT process callback): [`try_emit_error`]
//! - Caller must not miss the error: [`emit_error`] (blocks until callback available)

use std::sync::{Mutex, TryLockError};

use crate::Error;

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
#[allow(dead_code)]
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

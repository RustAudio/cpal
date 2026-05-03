use crate::Error;
use std::sync::{Mutex, TryLockError};

/// Deliver an error that the app must not miss, blocking if the callback is currently executing on
/// another thread. Use this for fatal or actionable errors.
pub(crate) fn emit_error<E>(callback: &Mutex<E>, error: Error)
where
    E: FnMut(Error) + Send + ?Sized,
{
    let mut cb = callback.lock().unwrap_or_else(|e| e.into_inner());
    (*cb)(error);
}

/// Try to deliver an error without blocking the caller.
///
/// Returns `Ok(())` if the callback was invoked, or `Err(error)` returning the error if the lock
/// was contended and the error could not be delivered.
///
/// Use this on real-time threads where blocking or heap-allocating (including logging) is not
/// acceptable.
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

/// Best-effort error delivery helper.
///
/// Calls [`try_emit_error`]. If the lock is contended and the error cannot be delivered, emits a
/// warning-level message (when the `log` feature is enabled) and discards the error.
///
/// Use this on non-real-time threads for errors where dropping a notification is acceptable but
/// should be observable.
pub(crate) fn emit_error_or_warn<E>(callback: &Mutex<E>, error: Error)
where
    E: FnMut(Error) + Send + ?Sized,
{
    if let Err(_e) = try_emit_error(callback, error) {
        #[cfg(feature = "log")]
        log::warn!("cpal: {} (error callback busy; notification dropped)", _e);
    }
}

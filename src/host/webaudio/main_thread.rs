//! Utility for invoking functions on the browser's main thread.
//! This allows for creating audio contexts within workers,
//! when normally having access to the window object is required.

#[cfg(target_os = "emscripten")]
pub use self::emscripten::*;

#[cfg(target_os = "unknown")]
pub use self::unknown::*;

/// Proxying implementation for `wasm32-unknown-emscripten`: will send
/// functions through Emscripten's queue to run on the main thread.
#[cfg(target_os = "emscripten")]
mod emscripten {
    use crate::{Error, ErrorKind};
    use std::ffi::c_void;

    // Functions provided by `emscripten/proxying.h` and `emscripten/threading.h`
    unsafe extern "C" {
        /// Returns true if the current thread is the thread that hosts the Emscripten
        /// runtime.
        fn emscripten_is_main_runtime_thread() -> bool;

        /// Returns the thread ID of the thread that hosts the Emscripten runtime.
        fn emscripten_main_runtime_thread_id() -> usize;

        /// Get the queue used for proxying low-level runtime work.
        fn emscripten_proxy_get_system_queue() -> *mut c_void;

        /// Enqueue `func` to be called with argument `arg` on the given queue
        /// and thread then wait for `func` to be executed synchronously before returning.
        fn emscripten_proxy_sync(
            queue: *mut c_void,
            target_thread: usize,
            func: extern "C" fn(*mut c_void),
            arg: *mut c_void,
        ) -> bool;
    }

    /// Runs `func` on the browser main thread.
    pub fn try_run<F, R>(func: F) -> Result<R, Error>
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        /// Data that gets sent between the two browser threads.
        struct SyncSlot<F, R> {
            /// The function to execute.
            func: Option<F>,
            /// The value that was returned.
            ret: Option<R>,
        }

        /// Callback that gets invoked on the main thread.
        extern "C" fn trampoline<F, R>(arg: *mut c_void)
        where
            F: FnOnce() -> R,
        {
            // SAFETY: `arg` points at a `SyncSlot` on the calling thread's stack.
            // `emscripten_proxy_sync` keeps that thread blocked until this returns,
            // so the pointer stays valid and unaliased for the call.
            let slot = unsafe { &mut *arg.cast::<SyncSlot<F, R>>() };
            let func = slot.func.take().expect("proxied task ran twice");
            slot.ret = Some(func());
        }

        // SAFETY: `func` and its return value are Send and can be moved
        // between threads. `emscripten_proxy_sync` guarantees that it
        // will invoke `trampoline` and not return until it is finished.
        unsafe {
            if emscripten_is_main_runtime_thread() {
                return Ok(func());
            }

            let mut slot = SyncSlot {
                func: Some(func),
                ret: None,
            };

            emscripten_proxy_sync(
                emscripten_proxy_get_system_queue(),
                emscripten_main_runtime_thread_id(),
                trampoline::<F, R>,
                (&raw mut slot).cast(),
            );

            slot.ret.ok_or_else(|| {
                Error::with_message(ErrorKind::BackendError, "proxied task did not run")
            })
        }
    }
}

/// Proxying implementation for `wasm32-unknown-unknown`: will check to see
/// if closures are running on the main thread, and fail if ever called from a worker.
#[cfg(target_os = "unknown")]
mod unknown {
    use crate::{Error, ErrorKind};

    /// Attempts to run `func`. If this was not already the main browser thread,
    /// then returns [`Err`] because proxying is not possible on this target.
    pub fn try_run<F, R>(func: F) -> Result<R, Error>
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        if web_sys::window().is_some() {
            Ok(func())
        } else {
            Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "cannot perform webaudio operations on a worker thread",
            ))
        }
    }
}

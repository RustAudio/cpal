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

    /// Run `func` on the browser main thread and return its result, blocking
    /// the caller until it completes. Runs inline when the caller already is
    /// the main thread.
    pub fn run<F, R>(func: F) -> R
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
            let func = slot.func.take().expect("proxied task run twice");
            slot.ret = Some(func());
        }

        // SAFETY: `func` and its return value are Send and can be moved
        // between threads. `emscripten_proxy_sync` guarantees that it
        // will invoke `trampoline` and not return until it is finished.
        unsafe {
            if emscripten_is_main_runtime_thread() {
                return func();
            }

            let mut slot = SyncSlot {
                func: Some(func),
                ret: None,
            };

            let ok = emscripten_proxy_sync(
                emscripten_proxy_get_system_queue(),
                emscripten_main_runtime_thread_id(),
                trampoline::<F, R>,
                (&raw mut slot).cast(),
            );

            assert!(
                ok,
                "emscripten_proxy_sync to the browser main thread failed"
            );
            slot.ret.take().expect("proxied task did not run")
        }
    }

    /// Runs `func` on the browser main thread. For the Emscripten target,
    /// always succeeds with [`Some`].
    pub fn try_run<F, R>(func: F) -> Option<R>
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        Some(run(func))
    }
}

/// Proxying implementation for `wasm32-unknown-unknown`: will check to see
/// if closures are running on the main thread, and fail if ever called from a worker.
#[cfg(target_os = "unknown")]
mod unknown {
    /// Asserts that this is the main browser thread and runs `func`.
    pub fn run<F, R>(func: F) -> R
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        assert!(
            is_main_thread(),
            "proxying closures is not supported on wasm32-unknown-unknown"
        );
        func()
    }

    /// Attempts to run `func`. If this was not already the main browser thread,
    /// then returns [`None`] because proxying is not possible on this target.
    pub fn try_run<F, R>(func: F) -> Option<R>
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        if is_main_thread() {
            Some(run(func))
        } else {
            None
        }
    }

    /// Whether this is the main browser thread.
    fn is_main_thread() -> bool {
        web_sys::window().is_some()
    }
}

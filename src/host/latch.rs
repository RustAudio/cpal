//! Stream-readiness latch used by backends with dedicated worker threads.
//!
//! Prevents worker threads from invoking user callbacks before the `Stream` handle has been
//! returned to the caller.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    thread::Thread,
};

/// Signals worker threads that the stream handle has been given to the caller.
#[derive(Debug)]
pub(crate) struct Latch {
    /// `Option` so `Drop` can move it out before unparking, closing the window where a thread
    /// could wake, see the Arc still alive (flag=false), re-park, then be orphaned.
    flag: Option<Arc<AtomicBool>>,
    threads: Vec<Thread>,
}

/// Held by a worker thread. Parks until the matching [`Latch`] is released.
#[derive(Clone, Debug)]
pub(crate) struct LatchWaiter(Weak<AtomicBool>);

impl Latch {
    /// Creates a new stream-readiness latch.
    pub(crate) fn new() -> Self {
        Self {
            flag: Some(Arc::new(AtomicBool::new(false))),
            threads: Vec::new(),
        }
    }

    /// Returns a waiter that unblocks when this latch is released.
    pub(crate) fn waiter(&self) -> LatchWaiter {
        LatchWaiter(Arc::downgrade(
            self.flag
                .as_ref()
                .expect("waiter called on a dropped Latch"),
        ))
    }

    /// Registers a thread to be unparked when [`release`](Self::release) is called.
    #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
    ))]
    pub(crate) fn add_thread(&mut self, thread: Thread) {
        self.threads.push(thread);
    }

    /// Releases the latch and unparks all registered threads.
    pub(crate) fn release(&self) {
        if let Some(flag) = &self.flag {
            flag.store(true, Ordering::Release);
        }
        for t in &self.threads {
            t.unpark();
        }
    }
}

impl Drop for Latch {
    fn drop(&mut self) {
        // Invalidate the Arc *before* unparking so waiters see upgrade() == None and exit cleanly
        // on the error path (latch dropped without being released).
        drop(self.flag.take());
        for t in &self.threads {
            t.unpark();
        }
    }
}

impl LatchWaiter {
    /// Parks the calling thread until the latch is released or dropped without releasing.
    ///
    /// Returns `true` if the stream is ready, `false` if the [`Latch`] was dropped before release.
    #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
    ))]
    pub(crate) fn wait(&self) -> bool {
        loop {
            match self.0.upgrade() {
                None => return false,
                Some(flag) if flag.load(Ordering::Acquire) => return true,
                Some(flag) => {
                    drop(flag); // release strong ref before parking
                    std::thread::park();
                }
            }
        }
    }

    /// Returns `true` if the latch has already been released.
    #[cfg(all(target_vendor = "apple", not(target_os = "macos")))]
    pub(crate) fn is_released(&self) -> bool {
        self.0.upgrade().is_some_and(|f| f.load(Ordering::Acquire))
    }
}

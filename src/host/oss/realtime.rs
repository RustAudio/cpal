//! Realtime promotion for OSS threads
//!
//! FreeBSD priority numbering is mixed,
//! due to rtprio and POSIX.
//!
//! Threads are created as FIFO, rather
//! than REALTIME, to avoid them getting
//! interrupted mid execution.

// [TODO]: I really need to get round to fixing that spinning
// on fatal error issue on worker threads, starving the system isn't
// exactly being a considerate programmer.
use crate::{Error, ErrorKind};

/// The scheduling priority value for a realtime promoted thread.
///
/// FreeBSD's `rtprio(2)`:                   0(urgent) -> 31(lowest)
/// POSIX `sched_*`/`pthread_setschedparam`: 0(lowest) -> 31(urgent)
///
/// This uses the POSIX method.
/// 16 is hardcoded because it's a safe middle ground.
const RT_THREAD_SCHED_PRIORITY: libc::c_int = 16;

/// Raises the calling thread to real-time scheduling.
pub(super) fn promote_current_thread() -> Result<(), Error> {
    let param = libc::sched_param {
        sched_priority: RT_THREAD_SCHED_PRIORITY,
    };

    // SAFETY: `param` outlives the call, and `pthread_self()` is always a valid handle.
    // `pthread_setschedparam` returns the errno value directly and does not set `errno`.
    let rc = unsafe { libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param) };
    if rc != 0 {
        return Err(Error::with_message(
            ErrorKind::RealtimeDenied,
            format!(
                "Failed to promote audio thread to real-time priority: {}",
                std::io::Error::from_raw_os_error(rc)
            ),
        ));
    }
    Ok(())
}

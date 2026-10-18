use crate::StreamInstant;

/// Returns the current time using the global stream clock.
/// 
/// The clock in question is `CLOCK_MONOTONIC`, rationale:
/// 
/// `CLOCK_MONOTONIC`: The default clock, used by std::time::Instant
/// `CLOCK_MONOTONIC_PRECISE`: more precise but costs more per call
/// `CLOCK_MONOTONIC_FAST/COARSE`: Cheaper, but far coarser
/// `CLOCK_REALTIME`: Can go backwards as it's the wall clock
/// 
/// [FreeBSD Clock Docs](https://man.freebsd.org/cgi/man.cgi?clock_gettime)
pub(super) fn now() -> StreamInstant {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };

    let r = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    debug_assert_eq!(r, 0); // `clock_gettime` should never fail

    StreamInstant::new(ts.tv_nsec as u64, ts.tv_nsec as u32)
}
use super::syscall::{
    AudioBufInfo, SNDCTL_DSP_GETISPACE, SNDCTL_DSP_GETODELAY, ioctl_int, ioctl_ptr,
};
use crate::StreamInstant;
use std::{fs::File, time::Duration};

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
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };

    let r = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    debug_assert_eq!(r, 0); // `clock_gettime` should never fail

    StreamInstant::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

// Should be practically infallible, but any errors are handled via a delay of 0
pub(super) fn device_timestamp(
    fd: &File,
    callback_ts: StreamInstant,
    bytes_per_sec: u64,
    input: bool,
) -> StreamInstant {
    let delay_bytes = match input {
        true => {
            let mut info = AudioBufInfo::default();
            ioctl_ptr(fd, SNDCTL_DSP_GETISPACE, &mut info)
                .map(|_| info.bytes.max(0) as u64)
                .unwrap_or(0)
        }
        false => {
            // [TODO] [RESEARCH] I believe this is actually until the END of the buffer, not the first sample
            ioctl_int(fd, SNDCTL_DSP_GETODELAY, 0)
                .map(|v| v as u64)
                .unwrap_or(0)
        }
    };

    let delay = Duration::from_secs_f64(delay_bytes as f64 / bytes_per_sec as f64);
    match input {
        true => callback_ts.checked_sub(delay).unwrap_or(callback_ts),
        false => callback_ts.checked_add(delay).unwrap_or(callback_ts),
    }
}

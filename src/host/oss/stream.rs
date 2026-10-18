//! Stream creation, and lifetime handling for OSS
//! 
//! The current approach uses a blocking thread to handle the FD,
//! its lifetime cycle for output looks roughly like this:
//! 
//! loop {
//!     wait if paused; exit if stopping
//!     zero the buffer (equilibrium)
//!     stamp callback/device tses
//!     call the user supplied data callback
//!     `write()` the whole buffer, handling partial writes and `EINTR`
//!     on error: call the error callback
//!     on a fatal error: call the error callback and stop
//! }

use crate::{
    host::{
        equilibrium::fill_equilibrium,
        latch::Latch,
    },
    traits::StreamTrait,
    StreamTimestamp,
    StreamInstant,
    CallbackInfo,
    SampleFormat,
    FrameCount,
    ErrorKind,
    Error,
    Data,
};

use super::{
    probe::ioctl_int,
    error_from_io,
    formats,
    consts,
};

use std::{
    thread::JoinHandle,
    os::fd::AsRawFd,
    time::Duration,
    io::Write,
    fs::File,
    sync::{
        Condvar,
        Mutex,
        Arc,
        atomic::{
            AtomicBool,
            Ordering,
        },
    },
};

/// [TODO] ::new_input/output/duplex
pub struct Stream {
    pub(super) control: Arc<Control>,
    pub(super) worker: Option<JoinHandle<()>>,
    pub(super) latch: Latch
}

unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    fn pause(&self) -> Result<(), Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    /// Returns the current time using the same clock
    /// as the stream.
    /// 
    /// The clock in question is `CLOCK_MONOTONIC`, rationale:
    /// 
    /// `CLOCK_MONOTONIC`: The default clock, used by std::time::Instant
    /// `CLOCK_MONOTONIC_PRECISE`: more precise but costs more per call
    /// `CLOCK_MONOTONIC_FAST/COARSE`: Cheaper, but far coarser
    /// `CLOCK_REALTIME`: Can go backwards as wall clock
    /// 
    /// [FreeBSD Clock Docs](https://man.freebsd.org/cgi/man.cgi?clock_gettime)
    fn now(&self) -> StreamInstant {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };

        let r = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        debug_assert_eq!(r, 0); // `clock_gettime` should never fail

        StreamInstant::new(ts.tv_nsec as u64, ts.tv_nsec as u32)
    }
}


/// [TODO]: use this in Stream::now() aswell
fn get_streaminstant() -> StreamInstant {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };

    let r = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    debug_assert_eq!(r, 0); // `clock_gettime` should never fail

    StreamInstant::new(ts.tv_nsec as u64, ts.tv_nsec as u32)
}

#[derive(Default)]
pub(super) struct Control {
    stop: AtomicBool,
    paused: Mutex<bool>,
    cv: Condvar,
}

// no moving into another thread currently so data_callback is required as mut
pub(super) fn run_output<D, E>(fd: File, ctl: Arc<Control>, period_bytes: usize, mut data_callback: D, error_callback: E, sample_format: SampleFormat)
where
    D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
    E: FnMut(Error) + Send + 'static,
{
    let mut buf = vec![0u8; period_bytes];
    let mut filled = 0;

    loop {
        if ctl.stop.load(Ordering::Acquire) { break; }

        // Park the thread if the stream is currently paused (re-checking stop when woken)
        {
            let mut p = ctl.paused.lock().unwrap();
            while *p && !ctl.stop.load(Ordering::Acquire) {
                p = ctl.cv.wait(p).unwrap();
            }
        }
        if ctl.stop.load(Ordering::Acquire) { break; }

        if filled == 0 {
            let callback_ts = get_streaminstant();
            let device_ts = get_streaminstant(); // [TODO] + delay;
            fill_equilibrium(&mut buf, sample_format);

            // [TODO] safety note
            let samples = period_bytes / sample_format.sample_size();
            let mut data = unsafe { Data::from_parts(buf.as_mut_ptr() as *mut (), samples, sample_format) };
            let info = CallbackInfo::new(StreamTimestamp { callback: callback_ts, device: device_ts }, false); // [TODO] handle xruns
            // [TODO] add documentation to when OSS xruns are reported relative to the callback
            data_callback(&mut data, &info)
        }

        // wait untill the device can accept data
        let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, 50) } ;
        if r < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted { continue; } else {
                // error callback
            }
        }
        if r == 0 { continue; } // timeout: loop around and re-check flags

        match (&fd).write(&buf[filled..]) {
            Ok(n) => {
                filled += n;
                if filled == buf.len() {
                    filled = 0;
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock { continue; }
                if e.kind() == std::io::ErrorKind::Interrupted { continue; }
                // error callback
            }
        }
    }
}

use libc::c_int;

/// The driver may round the rate by a Hz or so (e.g. 48000 -> 47999).
const RATE_TOLERANCE_HZ: c_int = 2;

fn rate_close_enough(asked: c_int, got: c_int) -> bool {
    (asked - got).abs() <= RATE_TOLERANCE_HZ
}

const SNDCTL_DSP_GETBLKSIZE: libc::c_ulong = 0xC004_5004;  // _IOWR('P', 4, int)
const SNDCTL_DSP_SETFRAGMENT: libc::c_ulong = 0xC004_500A; // _IOWR('P', 10, int)

pub(super) fn configure(
    fd: &File,
    config: &crate::StreamConfig,
    fmt: crate::SampleFormat,
) -> Result<usize, Error> {
    let Some(afmt) = formats::afmt_from_sample_format(fmt) else { return Err(Error::new(ErrorKind::UnsupportedConfig)); };
    let ch = config.channels as libc::c_int;
    let rate = config.sample_rate as libc::c_int;

    // Fragment size first: classic OSS sizes the buffer when the format is fixed.
    if let crate::BufferSize::Fixed(frames) = config.buffer_size {
        let bytes = frames as usize * config.channels as usize * fmt.sample_size();
        let log2 = bytes.next_power_of_two().trailing_zeros() as libc::c_int;
        let arg = (2 << 16) | log2; // 2 fragments, 2^log2 bytes each
        let _ = ioctl_int(fd, SNDCTL_DSP_SETFRAGMENT, arg); // a hint, so ignore failure
    }

    // Each call writes back what the driver actually chose: reject mismatches.
    if ioctl_int(fd, consts::SNDCTL_DSP_SETFMT, afmt).map_err(error_from_io)? != afmt {
        return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("Format: {fmt}/{afmt} was rejected")));
    }
    if ioctl_int(fd, consts::SNDCTL_DSP_CHANNELS, ch).map_err(error_from_io)? != ch {
        return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("Channels: {ch} was rejected")));
    }
    // if ioctl_int(fd, consts::SNDCTL_DSP_SPEED, rate).map_err(error_from_io)? != rate {
    //     return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("Sample Rate: {rate} was rejected")));
    // }
    let got = ioctl_int(fd, consts::SNDCTL_DSP_SPEED, rate).map_err(error_from_io)?;
    if !rate_close_enough(rate, got) {
        return Err(Error::with_message(
            ErrorKind::UnsupportedConfig,
            format!("SPEED: asked for {rate}, driver gave {got}"),
        ));
    }

    // Read back the real block size
    let blk = ioctl_int(fd, SNDCTL_DSP_GETBLKSIZE, 0).map_err(|e| {
        Error::with_message(
            ErrorKind::UnsupportedConfig, // use whichever kind fits
            format!(
                "GETBLKSIZE failed: {e} (errno {:?}, request {:#010x}, fd {})",
                e.raw_os_error(),
                SNDCTL_DSP_GETBLKSIZE,
                fd.as_raw_fd(),
            ),
        )
    })?;
    Ok(blk.max(1) as usize)
}
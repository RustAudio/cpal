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
    host::equilibrium::fill_equilibrium,
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

use std::{
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

pub struct Stream {}

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

struct Control {
    stop: AtomicBool,
    paused: Mutex<bool>,
    cv: Condvar,
}

// no moving into another thread currently so data_callback is required as mut
fn run_output<D, E>(fd: File, ctl: Arc<Control>, period_bytes: usize, mut data_callback: D, error_callback: E, sample_format: SampleFormat)
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
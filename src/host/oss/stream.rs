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
//! 
//! [TODO]: High-priority threads.

use std::{
    thread::JoinHandle,
    io::{Read, Write},
    os::fd::AsRawFd,
    time::Duration,
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

use crate::{
    host::{
        equilibrium::fill_equilibrium,
        latch::Latch,
    },
    traits::StreamTrait,
    DuplexCallbackInfo,
    DuplexStreamConfig,
    StreamTimestamp,
    StreamInstant,
    CallbackInfo,
    StreamConfig,
    SampleFormat,
    FrameCount,
    BufferSize,
    ErrorKind,
    Error,
    Data,
};

use super::{
    syscall::{self, AudioBufInfo, ioctl_int, ioctl_ptr},
    error_from_io,
    formats,
    clock,
};

use libc::c_int;


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
        *self.control.paused.lock().unwrap() = false;
        self.control.cv.notify_all();
        Ok(())
    }

    fn pause(&self) -> Result<(), Error> {
        *self.control.paused.lock().unwrap() = true;
        self.control.cv.notify_all();
        Ok(())
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error> {
        {
            // hold the mutex across both store and notify
            let _p = self.control.paused.lock().unwrap();
            self.control.stop.store(true, Ordering::Release);
            self.control.cv.notify_all();
        }
        Ok(()) // join happens in Drop; poll timeout bounds it to <= 50 ms
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    /// Returns the current time using the same clock
    /// as the stream.
    /// 
    /// The clock in question is `CLOCK_MONOTONIC`.
    fn now(&self) -> StreamInstant {
        clock::now()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        {
            let _p = self.control.paused.lock().unwrap();
            self.control.stop.store(true, Ordering::Release);
            self.control.cv.notify_all();
        }
        if let Some(h) = self.worker.take() {
            let _ = h.join(); // returns within ~50 ms (poll timeout)
        }
    }
}

pub(super) struct Control {
    stop: AtomicBool,
    paused: Mutex<bool>,
    cv: Condvar,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            // explicitely set default values (for some)
            stop: AtomicBool::new(false),
            paused: Mutex::new(true), // Start the stream paused so that the user must first call `stream.start()`
            cv: Condvar::new(),
        }
    }
}

/// The loop for reading input data
pub(super) fn input_loop<D, E>(fd: File, ctl: Arc<Control>, period_bytes: usize, mut data_callback: D, mut error_callback: E, sample_format: SampleFormat)
where
    D: FnMut(&Data, &CallbackInfo) + Send + 'static,
    E: FnMut(Error               ) + Send + 'static,
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

        // wait until the device has data to be read
        let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, 50) };
        if r < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted { continue; } else {
                error_callback(error_from_io(e));
            }
        }
        if r == 0 { continue; } // timeout: loop around and re-check flags

        match (&fd).read(&mut buf[filled..]) {
            Ok(0) => {
                // /dev/dspX returning EOF means the device was removed
                error_callback(Error::new(ErrorKind::DeviceNotAvailable));
                break;
            }
            Ok(n) => {
                filled += n;
                if filled == buf.len() {
                    filled = 0;

                    let callback_ts = clock::now();
                    let device_ts   = clock::now(); // [TODO] - delay

                    let samples = period_bytes / sample_format.sample_size();
                    // SAFETY: `buf` is valid for `period_bytes`, and the callback only
                    // borrows it (via `&Data`) for the duration of this call.
                    let data = unsafe { Data::from_parts(buf.as_mut_ptr() as *mut (), samples, sample_format) };
                    let info = CallbackInfo::new(StreamTimestamp { callback: callback_ts, device: device_ts }, false); // [TODO]: Report xruns

                    data_callback(&data, &info);
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock { continue; }
                if e.kind() == std::io::ErrorKind::Interrupted { continue; }
                error_callback(error_from_io(e));
            }
        }
    }
}

/// The loop for writing output data
pub(super) fn output_loop<D, E>(fd: File, ctl: Arc<Control>, period_bytes: usize, mut data_callback: D, mut error_callback: E, sample_format: SampleFormat)
where
    D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
    E: FnMut(Error                   ) + Send + 'static,
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
            let callback_ts = clock::now();
            let device_ts = clock::now(); // [TODO] + delay;
            fill_equilibrium(&mut buf, sample_format);

            // [TODO] safety note
            let samples = period_bytes / sample_format.sample_size();
            let mut data = unsafe { Data::from_parts(buf.as_mut_ptr() as *mut (), samples, sample_format) };
            let info = CallbackInfo::new(StreamTimestamp { callback: callback_ts, device: device_ts }, false); // [TODO] report xruns
            // [TODO] add documentation to when OSS xruns are reported relative to the callback
            data_callback(&mut data, &info);
        }

        // wait untill the device can accept data
        let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, 50) } ;
        if r < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted { continue; } else {
                // error callback
                error_callback(error_from_io(e));
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
                error_callback(error_from_io(e));
            }
        }
    }
}

/// The loop for reading and writing duplex data [TODO]
pub(super) fn duplex_loop<D, E>(fd: File, ctl: Arc<Control>, in_period_bytes: usize, out_period_bytes: usize, mut data_callback: D, mut error_callback: E, sample_format: SampleFormat)
where
    D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
    E: FnMut(Error                                ) + Send + 'static,
{
    let mut in_buf  = vec![0u8; in_period_bytes];
    let mut in_filled = 0;
    let mut out_buf = vec![0u8; out_period_bytes];

    // Prime the playback ring with one period of silence. Otherwise the first
    // read + callback delays the first write and the device underruns immediately.
    fill_equilibrium(&mut out_buf, sample_format);
    // for _ in 0..fragstotal
    for _ in 0..4 {
        let mut primed = 0;
        while primed < out_buf.len() {
            if ctl.stop.load(Ordering::Acquire) { return; }

            let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
            let r = unsafe { libc::poll(&mut pfd, 1, 50) };
            if r < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted { continue; }
                error_callback(error_from_io(e));
            }
            if r == 0 { continue; }

            match (&fd).write(&out_buf[primed..]) {
                Ok(n) => primed += n,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock  { continue; }
                    if e.kind() == std::io::ErrorKind::Interrupted { continue; }

                    error_callback(error_from_io(e));
                    break;
                }
            }
        }
    }

    loop {
        if ctl.stop.load(Ordering::Acquire) { break; }

        {
            let mut p = ctl.paused.lock().unwrap();
            while *p && !ctl.stop.load(Ordering::Acquire) {
                p = ctl.cv.wait(p).unwrap();
            }
        }
        if ctl.stop.load(Ordering::Acquire) { break; }

        // Accumulate a full input period
        while in_filled < in_buf.len() {
            if ctl.stop.load(Ordering::Acquire) { return; }

            let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let r = unsafe { libc::poll(&mut pfd, 1, 50) };
            if r < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted { continue; }
                error_callback(error_from_io(e));
                continue;
            }
            if r == 0 { continue; } // timeout: outer loop re-checks pause/stop next pass

            match (&fd).read(&mut in_buf[in_filled..]) {
                Ok(0) => {
                    error_callback(error_from_io(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)));
                    return;
                }
                Ok(n) => in_filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock  => continue,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => { error_callback(error_from_io(e)); continue; }
            }
        }

        // user callback populates the output period
        let callback_ts = clock::now();
        let device_ts   = clock::now(); // [TODO] - delay
        fill_equilibrium(&mut out_buf, sample_format);

        let in_samples  = in_period_bytes / sample_format.sample_size();
        let out_samples = out_period_bytes / sample_format.sample_size();

        // SAFETY: Both buffers live across the callback, the callback only borrows them
        // (read-only for input via `&Data`, read-write for output via `&mut Data`)
        let in_data      = unsafe { Data::from_parts(in_buf .as_ptr()     as *mut (), in_samples,  sample_format) };
        let mut out_data = unsafe { Data::from_parts(out_buf.as_mut_ptr() as *mut (), out_samples, sample_format) };
        let info = DuplexCallbackInfo::new(
            // [TODO]: report xruns
             CallbackInfo::new(StreamTimestamp { callback: callback_ts, device: device_ts }, false),
            CallbackInfo::new(StreamTimestamp { callback: callback_ts, device: device_ts }, false),
        );
        
        data_callback(&in_data, &mut out_data, &info);

        in_filled = 0;

        // fully drain the output period
        let mut written = 0;
        while written < out_buf.len() {
            if ctl.stop.load(Ordering::Acquire) { return; }

            let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
            let r = unsafe { libc::poll(&mut pfd, 1, 50) };
            if r < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted { continue; }
                
                error_callback(error_from_io(e));
                continue;
            }
            if r == 0 { continue; }

            match (&fd).write(&out_buf[written..]) {
                Ok(n) => written += n,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock  { continue; }
                    if e.kind() == std::io::ErrorKind::Interrupted { continue; }

                    error_callback(error_from_io(e));
                    continue;
                }
            }
        }
    }
}

/// The driver may round the rate by a Hz or so (e.g. 48000 -> 47999).
/// 
/// [NOTE]: This only exists because in my own testing, whenever I tried to
/// open a 48kHz stream, it always gave me 47999Hz.
const RATE_TOLERANCE_HZ: c_int = 2;

fn rate_close_enough(asked: c_int, got: c_int) -> bool {
    (asked - got).abs() <= RATE_TOLERANCE_HZ
}

pub(super) fn configure(
    fd: &File,
    config: &StreamConfig,
    fmt: SampleFormat,
    input: bool, // a bool here as it should only be input/output, use `configure_duplex` otherwise. [TODO] unify them?
) -> Result<usize, Error> {
    let Some(afmt) = formats::afmt_from_sample_format(fmt) else { return Err(Error::new(ErrorKind::UnsupportedConfig)); };
    let ch = config.channels as c_int;
    let rate = config.sample_rate as c_int;

    // Fragment size first: classic OSS sizes the buffer when the format is fixed.
    if let BufferSize::Fixed(frames) = config.buffer_size {
        let bytes = frames as usize * config.channels as usize * fmt.sample_size();
        let log2 = bytes.next_power_of_two().trailing_zeros() as c_int;
        let arg = (2 << 16) | log2; // 2 fragments, 2^log2 bytes each
        let _ = ioctl_int(fd, syscall::SNDCTL_DSP_SETFRAGMENT, arg); // a hint, so ignore failure
    }

    // Each call writes back what the driver actually chose: reject mismatches.
    if ioctl_int(fd, syscall::SNDCTL_DSP_SETFMT, afmt).map_err(error_from_io)? != afmt {
        return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("Format: {fmt}/{afmt} was rejected")));
    }
    if ioctl_int(fd, syscall::SNDCTL_DSP_CHANNELS, ch).map_err(error_from_io)? != ch {
        return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("Channels: {ch} was rejected")));
    }
    // if ioctl_int(fd, consts::SNDCTL_DSP_SPEED, rate).map_err(error_from_io)? != rate {
    //     return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("Sample Rate: {rate} was rejected")));
    // }
    let got = ioctl_int(fd, syscall::SNDCTL_DSP_SPEED, rate).map_err(error_from_io)?;
    if !rate_close_enough(rate, got) {
        return Err(Error::with_message(
            ErrorKind::UnsupportedConfig,
            format!("Sample Rate: asked for {rate}, driver gave {got}"),
        ));
    }

    // Read back the real block size
    let mut info = AudioBufInfo::default();
    let req = match input {
        true => syscall::SNDCTL_DSP_GETISPACE,
        false => syscall::SNDCTL_DSP_GETOSPACE,
    };
    ioctl_ptr(fd, req, &mut info).map_err(error_from_io)?;
    Ok(info.fragsize.max(1) as usize) // not a good idea?
}

/// Configure an open device file descriptor
/// for a duplex stream.
/// 
/// [TODO]: hoist away some duplication between this and `configure`.
pub(super) fn configure_duplex(
    fd: &File,
    config: &DuplexStreamConfig,
    fmt: SampleFormat,
) -> Result<(usize, usize), Error> {
    let Some(afmt) = formats::afmt_from_sample_format(fmt) else {
        return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("OSS does not support {fmt}")));
    };
    let ch = config.output_channels as c_int;
    let rate = config.sample_rate as c_int;

    if config.input_channels != config.output_channels {
        return Err(Error::with_message(
            ErrorKind::UnsupportedConfig,
            "OSS duplex requires input_channels == output_channels",
        ));
    }

    if let BufferSize::Fixed(frames) = config.buffer_size {
        let bytes = frames as usize * config.output_channels as usize * fmt.sample_size();
        let log2 = bytes.next_power_of_two().trailing_zeros() as c_int;
        let arg = (2 << 16) | log2;
        let _ = ioctl_int(fd, syscall::SNDCTL_DSP_SETFRAGMENT, arg);
    }

    if ioctl_int(fd, syscall::SNDCTL_DSP_SETFMT, afmt).map_err(error_from_io)? != afmt {
        return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("Format: {fmt}/{afmt} was rejected")));
    }
    if ioctl_int(fd, syscall::SNDCTL_DSP_CHANNELS, ch).map_err(error_from_io)? != ch {
        return Err(Error::with_message(ErrorKind::UnsupportedConfig, format!("Channels: {ch} was rejected")));
    }
    let got = ioctl_int(fd, syscall::SNDCTL_DSP_SPEED, rate).map_err(error_from_io)?;
    if !rate_close_enough(rate, got) {
        return Err(Error::with_message(
            ErrorKind::UnsupportedConfig,
            format!("Sample Rate: asked for {rate}, driver gave {got}"),
        ));
    }

    let mut in_info = AudioBufInfo::default();
    ioctl_ptr(fd, syscall::SNDCTL_DSP_GETISPACE, &mut in_info).map_err(error_from_io)?;

    let mut out_info = AudioBufInfo::default();
    ioctl_ptr(fd, syscall::SNDCTL_DSP_GETOSPACE, &mut out_info).map_err(error_from_io)?;

    Ok((in_info.fragsize.max(1) as usize, out_info.fragsize.max(1) as usize))
}
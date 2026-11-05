//! Utilities for probing an audio device for its capabilities, formats etc.
//! This is not a general purpose OSS-spec host, each implementation varies in capabilities,
//! versions, specific quirks, and legacy/backwards compatability. This is for FreeBSD.
//!
//! # Notes
//!
//! ## Formats
//! - `SNDCTL_DSP_GETFMTS` (Sound-Control Digital-Signal-Processing Get-Formats): The bitmask of every format that the device supports.
//! - `SNDCTL_DSP_SETFMT` (Sound-Control Digital-Signal-Processing Set-Format) with a real `AFMT_*`: Requests that format and returns the one actually chosen.
//! - `SNDCTL_DSP_SETFMT` (Sound-Control Digital-Signal-Processing Set-Format) with `AFMT_QUERY`: Returns the one currently set.
//!
//! ## Capabilities
//! - `SNDCTL_DSP_GETCAPS` (Sound-Control Digital-Signal-Processing Get-Capabilities): Returns a bitmask of `DSP_CAP_*`/`PCM_CAP_*`
//!     - `PCM_CAP_INPUT`: The device can record
//!     - `PCM_CAP_OUTPUT`: The device can play
//!     - `PCM_CAP_DUPLEX`: The device supports full, native duplex, not just simplex
//!     - `PCM_CAP_MMAP`: Supports mem mapping the DMA buffer (unadvised)
//!     - `PCM_CAP_VIRTUAL`: A virtual (vchan?) device
//!     - `PCM_CAP_TRIGGER`: Supports `SNDCTL_DSP_SETTRIGGER`, meaning the stream can be explicitely start/stopped

use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    time::{Duration, Instant},
};

use crate::{Error, ErrorKind, SampleFormat, SupportedBufferSize, SupportedStreamConfigRange};

use super::{
    error_from_io, formats,
    stream::rate_close_enough,
    syscall::{
        self, AudioBufInfo, AudioErrInfo, SNDCTL_DSP_GETERROR, SNDCTL_DSP_GETISPACE,
        SNDCTL_DSP_GETOSPACE, ioctl_int, ioctl_ptr,
    },
};

#[derive(Clone, Copy)]
pub enum Mode {
    Read,
    Write,
    ReadWrite,
}

impl Mode {
    pub fn r(&self) -> bool {
        matches!(self, Mode::Read | Mode::ReadWrite)
    }

    pub fn w(&self) -> bool {
        matches!(self, Mode::Write | Mode::ReadWrite)
    }
}

/// Open a dsp file for ioctl
pub(super) fn open_dsp(path: &str, mode: Mode) -> std::io::Result<File> {
    OpenOptions::new()
        .read(mode.r())
        .write(mode.w())
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

/// A controlled blocking open of a dsp file for ioctl, with a given timeout.
///
/// This does *not* park the thread, it uses `O_NONBLOCK`, and retries, that way
/// a short lived OS thread is not spawned.
pub(super) fn blocking_open_dsp(
    path: &str,
    mode: Mode,
    timeout: Option<Duration>,
) -> Result<File, Error> {
    // dispath away to open_dsp if no timeout is given, this keeps
    // device.rs' build_*_streams cleaner and avoids a match.
    let Some(timeout) = timeout else {
        return open_dsp(path, mode).map_err(error_from_io);
    };

    let mut opts = OpenOptions::new();
    opts.read(mode.r())
        .write(mode.w())
        .custom_flags(libc::O_NONBLOCK);

    let start = Instant::now();
    loop {
        match opts.open(path) {
            Ok(f) => {
                return Ok(f);
            }
            Err(e) => {
                if matches!(e.raw_os_error(), Some(libc::EBUSY) | Some(libc::EAGAIN)) {
                    let elapsed = start.elapsed();
                    if elapsed >= timeout {
                        return Err(Error::with_message(
                            ErrorKind::ResourceExhausted,
                            "timed out waiting for dsp device to become available",
                        ));
                    }
                    let remaining = timeout - elapsed;
                    std::thread::sleep(remaining.min(Duration::from_millis(10)));
                } else {
                    // libc::EINTR
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(error_from_io(e));
                }
            }
        }
    }
}

/// Common sample rates used to probe a device with.
///
/// I tried swapping to cpal's own ones, but this caused issues.
/// trying to set a sample rate above the devices max rate caused:
///   - BackendError "Argument List too long (os error 7)" being returned
///   - "pcm0: feeder_build_rate(): can't set destination rate" being logged to the terminal
///
/// The end goal is to properly use cpal's common list, but first validate that every value is
/// within the device's reported supported min and max values, because:
///   - probing anyway would be wasteful and not 'correct'
///   - it causes things to be printed to the terminal, which is suboptimal
///
/// [TODO]: Migrate to cpal's actual `COMMON_SAMPLE_RATES`
const PROBE_RATES: &[i32] = &[
    8000, 16000, 22050, 32000, 44100, 48000, 88200, 96000, 192000,
];
// [TODO]: Actually clip to device's min/max channel counts
const PROBE_CHANNELS: &[i32] = &[1, 2, 4, 6, 8];

/// Probes a DSP devices for its supported configs by poking at it with various common configs.
///
/// [TODO]: I assume calling this/ioctl with Mode::ReadWrite would list duplex formats or fail if <=simplex?
pub(super) fn probe_configs(
    path: &str,
    mode: Mode,
) -> std::io::Result<Vec<SupportedStreamConfigRange>> {
    let mut out = Vec::new();
    let fmts = {
        let fd = open_dsp(path, mode)?;
        ioctl_int(&fd, syscall::SNDCTL_DSP_GETFMTS, 0)?
    };

    for (afmt, sample_format) in formats::FORMATS {
        // Early return if the device doesn't display that it supports it, no point trying
        if fmts & afmt == 0 {
            continue;
        }

        for &ch in PROBE_CHANNELS {
            let mut rates = Vec::new();
            let mut buffer_size = SupportedBufferSize::Unknown;

            for &rate in PROBE_RATES {
                // Fresh fd per attempt as OSS settings are sticky once the device is configured.
                let fd = open_dsp(path, mode)?;
                if ioctl_int(&fd, syscall::SNDCTL_DSP_SETFMT, *afmt)? != *afmt {
                    break;
                }
                if ioctl_int(&fd, syscall::SNDCTL_DSP_CHANNELS, ch)? != ch {
                    break;
                }
                let got = ioctl_int(&fd, syscall::SNDCTL_DSP_SPEED, rate)?;
                if rate_close_enough(rate, got) {
                    rates.push(rate as u32);

                    // Query the buffer size on the first working config for
                    // this (format, channels) pair. Buffer parameters depend
                    // only on format and channels on OSS, not on sample rate.
                    if rates.len() == 1 {
                        if let Some(frames) =
                            query_buffer_frames(&fd, mode, ch as u16, *sample_format)
                        {
                            buffer_size = SupportedBufferSize::Range {
                                min: frames,
                                max: frames,
                            };
                        }
                    }
                } else {
                    break;
                }
            }

            if let (Some(&min), Some(&max)) = (rates.first(), rates.last()) {
                out.push(SupportedStreamConfigRange::new(
                    ch as u16,
                    min,
                    max,
                    buffer_size,
                    *sample_format,
                ));
            }
        }
    }
    Ok(out)
}

/// Query the device's default buffer size (in frames) for the given direction.
///
/// Must be called on an fd that has already been configured as
/// OSS allocates the fragment buffer lazily on first configuration, so
/// querying on a freshly-opened fd returns zeros.
///
/// Returns `None` if the ioctl fails or the result doesn't divide cleanly
/// into whole frames.
fn query_buffer_frames(
    fd: &File,
    mode: Mode,
    channels: u16,
    sample_format: SampleFormat,
) -> Option<u32> {
    let mut info = AudioBufInfo::default();
    let req = match mode {
        Mode::Read => SNDCTL_DSP_GETISPACE,
        Mode::Write => SNDCTL_DSP_GETOSPACE,
        _ => {
            return None;
        } // This should never be used for duplex, atleast for now
    };
    ioctl_ptr(fd, req, &mut info).ok()?;

    let fragsize = usize::try_from(info.fragsize).ok()?;
    let fragstotal = usize::try_from(info.fragstotal).ok()?;
    let total_bytes = fragsize.checked_mul(fragstotal)?;

    let frame_size = channels as usize * sample_format.sample_size();
    if frame_size == 0 {
        return None;
    }

    let frames = total_bytes / frame_size;
    u32::try_from(frames).ok().filter(|&n| n > 0)
}

/// Reads and clears the xrun counters, returning `(playback underruns, capture overruns)`
/// since the last call.
///
/// Only call this from the thread that owns the stream, or two callers will steal each
/// other's counts.
///
/// This is cheap enough to call busily.
pub(super) fn get_xruns(f: &File) -> std::io::Result<(u32, u32)> {
    let mut info = AudioErrInfo::default();
    ioctl_ptr(f, SNDCTL_DSP_GETERROR, &mut info)?;
    Ok((
        info.play_underruns.max(0) as u32,
        info.rec_overruns.max(0) as u32,
    ))
}

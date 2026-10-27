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
    os::unix::fs::OpenOptionsExt,
};

use crate::{SampleFormat, SupportedBufferSize, SupportedStreamConfigRange};

use super::{
    formats,
    stream::rate_close_enough,
    syscall::{
        self, AudioBufInfo, SNDCTL_DSP_GETISPACE, SNDCTL_DSP_GETOSPACE, ioctl_int, ioctl_ptr,
    },
};

#[derive(Clone, Copy)]
pub enum Mode {
    Read,
    Write,
    ReadWrite,
}

/// Open a dsp file for ioctl
pub(super) fn open_dsp(path: &str, mode: Mode) -> std::io::Result<File> {
    let (r, w) = match mode {
        // [TODO]: Mode.r(), Mode.w()
        Mode::Read => (true, false),
        Mode::Write => (false, true),
        Mode::ReadWrite => (true, true),
    };
    OpenOptions::new()
        .read(r)
        .write(w)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
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
const PROBE_CHANNELS: &[i32] = &[1, 2, 4, 6, 8];

/// Probes a DSP devices for its supported configs by poking at it with various common configs.
///
/// [NOTE]: `SupportedBufferSize` is always `::Unknown`, due to OSS buffersize being a tad odd.
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

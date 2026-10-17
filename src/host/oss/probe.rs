//! Utilities for probing an audio device for its capabilities, formats etc.
//! 
//! The source of truth for much of this file is /usr/include/sys/soundcard.h from 
//! FreeBSD-15.1-RELEASE
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
    os::{
        unix::fs::OpenOptionsExt,
        fd::{
            OwnedFd,
            AsRawFd,
        }
    },
    fs::{
        OpenOptions,
        File,
    },
};

use crate::{
    SupportedStreamConfigRange,
    SupportedBufferSize,
    SampleFormat,
};

// FreeBSD ioctl encoding: IOC_OUT=0x4000_0000, IOC_INOUT=0xC000_0000,
// (size << 16) | (group << 8) | number. Group is 'P' (0x50), size is 4 (int).
const SNDCTL_DSP_SPEED:    libc::c_ulong = 0xC004_5002; // _IOWR('P', 2, int)
const SNDCTL_DSP_SETFMT:   libc::c_ulong = 0xC004_5005; // _IOWR('P', 5, int)
const SNDCTL_DSP_CHANNELS: libc::c_ulong = 0xC004_5006; // _IOWR('P', 6, int)
const SNDCTL_DSP_GETFMTS:  libc::c_ulong = 0x4004_500B; // _IOR('P', 11, int)

const AFMT_U8:     libc::c_int = 0x0000_0008;
const AFMT_S16_LE: libc::c_int = 0x0000_0010;
const AFMT_S32_LE: libc::c_int = 0x0000_1000;
const AFMT_FLOAT:  libc::c_int = 0x0000_4000;

#[derive(Clone, Copy)]
pub enum Mode { Read, Write, ReadWrite }

/// Open a dsp file for ioctl
fn open_dsp(path: &str, mode: Mode) -> std::io::Result<File> {
    let (r, w) = match mode {
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

/// [TODO]: cpal error returns?
fn ioctl_int(f: &File, req: libc::c_ulong, val: libc::c_int) -> std::io::Result<libc::c_int> {
    let mut v = val;
    match unsafe { libc::ioctl(f.as_raw_fd(), req, &mut v) } {
        -1 => Err(std::io::Error::last_os_error()),
        _ => Ok(v),
    }
}

const PROBE_RATES: &[i32] = &[8000, 16000, 22050, 32000, 44100, 48000, 88200, 96000, 192000];
const PROBE_CHANNELS: &[i32] = &[1, 2, 4, 6, 8];

/// Probes a DSP devices for its supported configs by poking at it with various common configs.
/// 
// [NOTE]: I assume calling this/ioctl with Mode::ReadWrite would list duplex formats or fail if <=simplex?
pub(super) fn probe_configs(path: &str, mode: Mode) -> std::io::Result<Vec<SupportedStreamConfigRange>> {
    let mut out = Vec::new();
    let fmts = {
        let fd = open_dsp(path, mode)?;
        ioctl_int(&fd, SNDCTL_DSP_GETFMTS, 0)?
    };

    for (afmt, sample_format) in [
        (AFMT_FLOAT,  SampleFormat::F32), // note: AFMT_FLOAT is a compatability shim
        (AFMT_S32_LE, SampleFormat::I32),
        (AFMT_S16_LE, SampleFormat::I16),
        (AFMT_U8,     SampleFormat::U8 ),
    ] {
        if fmts & afmt == 0 { continue; }
        for &ch in PROBE_CHANNELS {
            let mut rates = Vec::new();
            for &rate in PROBE_RATES {
                // Fresh fd per attempt as OSS settings are sticky once the device is configured.
                let fd = open_dsp(path, mode)?;
                if ioctl_int(&fd, SNDCTL_DSP_SETFMT, afmt)? != afmt { break; }
                if ioctl_int(&fd, SNDCTL_DSP_CHANNELS, ch)? != ch { break; }
                if ioctl_int(&fd, SNDCTL_DSP_SPEED, rate)? == rate { rates.push(rate as u32); }
            }
            if let (Some(&min), Some(&max)) = (rates.first(), rates.last()) {
                out.push(SupportedStreamConfigRange::new(
                    ch as u16, min, max, SupportedBufferSize::Unknown, sample_format,
                ));
            }
        }
    }
    Ok(out)
}
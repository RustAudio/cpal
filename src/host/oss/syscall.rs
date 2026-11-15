//! Utilities and constants for interfacing with FreeBSD syscalls.
//!
//! OSS ioctl request numbers use the BSD encoding from `<sys/ioccom.h>`:
//!
//! DDDSSSSSSSSSSSSSGGGGGGGGCCCCCCCC
//! 31-29  28-16     15-8     7-0
//! dir    size      group    command
//!
//! - **dir:** (3 bits) `IN` (bit 31, user -> kernel), `OUT` (bit 30, kernel -> user) or `VOID` (bit 29, no args).
//! - **size:** (13 bits) The argument's size in bytes, so the kernel knows how much to copy.
//! - **group:** (8 bits) The driver family. OSS DSP requests use `'P'` (0x50).
//! - **command:** (8 bits) The specific command within the group.
//!
//! The source of truth for much of this file is /usr/include/sys/soundcard.h from
//! FreeBSD-15.1-RELEASE

use libc::{c_char, c_int, c_uint, c_ulong};
use std::fs::File;
use std::os::fd::AsRawFd;

const IOC_VOID: c_ulong = 1 << 29;
const IOC_OUT: c_ulong = 1 << 30;
const IOC_IN: c_ulong = 1 << 31;
const IOC_INOUT: c_ulong = IOC_IN | IOC_OUT;
const IOCPARM_MASK: c_ulong = (1 << 13) - 1;
const IOCPARM_SHIFT: u32 = 16;
const IOCGROUP_SHIFT: u32 = 8;

const fn ioc(dir: c_ulong, group: u8, num: u8, size: usize) -> c_ulong {
    dir | ((size as c_ulong & IOCPARM_MASK) << IOCPARM_SHIFT)
        | ((group as c_ulong) << IOCGROUP_SHIFT)
        | num as c_ulong
}
const fn io(group: u8, num: u8) -> c_ulong {
    ioc(IOC_VOID, group, num, 0)
}
const fn ior<T>(group: u8, num: u8) -> c_ulong {
    ioc(IOC_OUT, group, num, size_of::<T>())
}
const fn iow<T>(group: u8, num: u8) -> c_ulong {
    ioc(IOC_IN, group, num, size_of::<T>())
}
const fn iowr<T>(group: u8, num: u8) -> c_ulong {
    ioc(IOC_INOUT, group, num, size_of::<T>())
}

pub(super) fn ioctl_int(f: &File, req: c_ulong, val: libc::c_int) -> std::io::Result<libc::c_int> {
    let mut v = val;
    match unsafe { libc::ioctl(f.as_raw_fd(), req, &mut v) } {
        -1 => Err(std::io::Error::last_os_error()),
        _ => Ok(v),
    }
}

pub(super) fn ioctl_ptr<T>(f: &File, req: c_ulong, arg: &mut T) -> std::io::Result<()> {
    // SAFETY: `arg` is a valid, aligned `T` for the whole call. `req` must be an ioctl
    // whose argument is a pointer to exactly one `T`.
    match unsafe { libc::ioctl(f.as_raw_fd(), req, arg as *mut T) } {
        -1 => Err(std::io::Error::last_os_error()),
        _ => Ok(()),
    }
}

// OSS DSP ioctls
const DSP: u8 = b'P'; // group for all SNDCTL_DSP_* requests
const OSSV4: u8 = b'X'; // group for newer requests like (some) SNDCTL_*

// command numbers within the group
const CMD_SPEED: u8 = 2;
const CMD_GETBLKSIZE: u8 = 4; // ! obselete
const CMD_SETFMT: u8 = 5;
const CMD_CHANNELS: u8 = 6;
const CMD_AUDIOINFO: u8 = 7;
const CMD_SETFRAGMENT: u8 = 10;
const CMD_GETFMTS: u8 = 11;
const CMD_GETOSPACE: u8 = 12;
const CMD_GETISPACE: u8 = 13;
const CMD_GETCAPS: u8 = 15;
const CMD_TRIGGER: u8 = 16; // `GETTRIGGER` and `SETTRIGGER` share it, just with opposite directions
const CMD_GETODELAY: u8 = 23;
const CMD_GETERROR: u8 = 25;

// FreeBSD ioctl encoding: IOC_OUT=0x4000_0000, IOC_INOUT=0xC000_0000,
// (size << 16) | (group << 8) | number. Group is 'P' (0x50), size is 4 (int).

// I refuse to fallback to using `GETBLKSIZE`,
// - it's officially obsolete
// - it is ambiguous as to its direction, when opening a duplex fd
// - freebsd reccomends using the modern `GETI/OSPACE`
// pub const SNDCTL_DSP_GETBLKSIZE:  c_ulong = ior ::<c_int>(DSP, CMD_GETBLKSIZE );
pub const SNDCTL_DSP_SPEED: c_ulong = iowr::<c_int>(DSP, CMD_SPEED);
pub const SNDCTL_DSP_SETFMT: c_ulong = iowr::<c_int>(DSP, CMD_SETFMT);
pub const SNDCTL_DSP_CHANNELS: c_ulong = iowr::<c_int>(DSP, CMD_CHANNELS);
pub const SNDCTL_DSP_SETFRAGMENT: c_ulong = iowr::<c_int>(DSP, CMD_SETFRAGMENT);
pub const SNDCTL_DSP_GETFMTS: c_ulong = ior::<c_int>(DSP, CMD_GETFMTS);
pub const SNDCTL_DSP_GETCAPS: c_ulong = ior::<c_int>(DSP, CMD_GETCAPS);
pub const SNDCTL_DSP_GETODELAY: c_ulong = ior::<c_int>(DSP, CMD_GETODELAY);
pub const SNDCTL_DSP_GETERROR: c_ulong = ior::<AudioErrInfo>(DSP, CMD_GETERROR);
pub const SNDCTL_DSP_GETTRIGGER: c_ulong = ior::<c_int>(DSP, CMD_TRIGGER);
pub const SNDCTL_DSP_SETTRIGGER: c_ulong = iow::<c_int>(DSP, CMD_TRIGGER);
pub const SNDCTL_DSP_GETOSPACE: c_ulong = ior::<AudioBufInfo>(DSP, CMD_GETOSPACE);
pub const SNDCTL_DSP_GETISPACE: c_ulong = ior::<AudioBufInfo>(DSP, CMD_GETISPACE);

pub const SNDCTL_AUDIOINFO: c_ulong = iowr::<OssAudioInfo>(OSSV4, CMD_AUDIOINFO);

// Capability Flags
pub const PCM_CAP_DUPLEX: c_int = 1 << 8;
pub const PCM_CAP_TRIGGER: c_int = 1 << 12;
pub const PCM_CAP_INPUT: c_int = 1 << 16;
pub const PCM_CAP_OUTPUT: c_int = 1 << 17;

/// FFI Mirror of OSS' `audio_buf_info`
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct AudioBufInfo {
    //                        | Output:          | Input:                  |
    pub fragments: c_int,  // | Fragments free   | Fragments ready to read |
    pub fragstotal: c_int, // |      Total fragments in the buffer         |
    pub fragsize: c_int,   // |           bytes per fragment               |
    pub bytes: c_int,      // | Bytes free       | Bytes ready to read     |
}

/// FFI Mirror of OSS' `audio_err_info`
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct AudioErrInfo {
    pub play_underruns: libc::c_int,
    pub rec_overruns: libc::c_int,
    pub play_ptradjust: libc::c_uint,
    pub rec_ptradjust: libc::c_uint,
    pub play_errorcount: libc::c_int,
    pub rec_errorcount: libc::c_int,
    pub play_lasterror: libc::c_int,
    pub rec_lasterror: libc::c_int,
    pub play_errorparm: libc::c_long,
    pub rec_errorparm: libc::c_long,
    pub filler: [libc::c_int; 16],
}

/// FFI Mirror of the OSS spec oss_audioinfo from FreeBSD's soundcard.h
///
/// FreeBSD doesn't fill this all out. Most notably, handle, which is
/// meant to be the persistent ID.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct OssAudioInfo {
    pub dev: c_int,
    pub name: [c_char; 64],
    pub _busy: c_int,
    pub _pid: c_int,
    pub _caps: c_int,
    pub _iformats: c_int,
    pub _oformats: c_int,
    pub _magic: c_int,
    pub _cmd: [c_char; 64],
    pub _card_number: c_int,
    pub _port_number: c_int,
    pub _mixer_dev: c_int,
    pub _legacy_device: c_int,
    pub _enabled: c_int,
    pub _flags: c_int,
    pub _min_rate: c_int,
    pub _max_rate: c_int,
    pub _min_channels: c_int,
    pub _max_channels: c_int,
    pub _binding: c_int,
    pub _rate_source: c_int,
    pub _handle: [c_char; 32],
    pub _nrates: c_uint,
    pub _rates: [c_uint; 20],
    pub _song_name: [c_char; 64],
    pub _label: [c_char; 16],
    pub _latency: c_int,
    pub _devnode: [c_char; 32],
    pub _next_play_engine: c_int,
    pub _next_rec_engine: c_int,
    pub _filler: [c_int; 184],
}

impl OssAudioInfo {
    pub fn zeroed() -> Self {
        // SAFETY: all bit patterns of all-zero bytes are valid for this POD type.
        unsafe { std::mem::zeroed() }
    }
    pub fn current_fd() -> Self {
        let mut info = Self::zeroed();
        info.dev = -1;
        info
    }

    pub fn name(&self) -> Option<String> {
        c_array_to_string(&self.name)
    }
}

fn c_array_to_string<const N: usize>(arr: &[c_char; N]) -> Option<String> {
    // SAFETY: c_char is i8 or u8 depending on target; casting to u8 is always sound.
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(arr.as_ptr() as *const u8, N) };
    std::ffi::CStr::from_bytes_until_nul(bytes)
        .ok()
        .map(|c| c.to_string_lossy().into_owned())
}

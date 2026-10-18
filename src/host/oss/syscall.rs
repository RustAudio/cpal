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

use libc::{c_int, c_ulong};
use std::os::fd::AsRawFd;
use std::fs::File;

const IOC_VOID:     c_ulong = 1 << 29;
const IOC_OUT:      c_ulong = 1 << 30;
const IOC_IN:       c_ulong = 1 << 31;
const IOC_INOUT:    c_ulong = IOC_IN | IOC_OUT;
const IOCPARM_MASK: c_ulong = (1 << 13) - 1;
const IOCPARM_SHIFT:  u32 = 16;
const IOCGROUP_SHIFT: u32 = 8;

const fn ioc(dir: c_ulong, group: u8, num: u8, size: usize) -> c_ulong {
    dir | ((size as c_ulong & IOCPARM_MASK) << IOCPARM_SHIFT)
        | ((group as c_ulong) << IOCGROUP_SHIFT)
        | num as c_ulong
}
const fn io     (group: u8, num: u8) -> c_ulong { ioc(IOC_VOID,  group, num, 0)        }
const fn ior <T>(group: u8, num: u8) -> c_ulong { ioc(IOC_OUT,   group, num, size_of::<T>()) }
const fn iow <T>(group: u8, num: u8) -> c_ulong { ioc(IOC_IN,    group, num, size_of::<T>()) }
const fn iowr<T>(group: u8, num: u8) -> c_ulong { ioc(IOC_INOUT, group, num, size_of::<T>()) }

// OSS DSP ioctls
const DSP: u8 = b'P'; // group for all SNDCTL_DSP_* requests

// command numbers within the group
const CMD_SPEED:       u8 = 2;
const CMD_GETBLKSIZE:  u8 = 4; // ! obselete
const CMD_SETFMT:      u8 = 5;
const CMD_CHANNELS:    u8 = 6;
const CMD_SETFRAGMENT: u8 = 10;
const CMD_GETFMTS:     u8 = 11;
const CMD_GETCAPS:     u8 = 15;
const CMD_TRIGGER:     u8 = 16; // `GETTRIGGER` and `SETTRIGGER` share it, just with opposite directions
const CMD_GETODELAY:   u8 = 23;
const CMD_GETOSPACE: u8 = 12;
const CMD_GETISPACE: u8 = 13;

pub const SNDCTL_DSP_SPEED:       c_ulong = iowr::<c_int>(DSP, CMD_SPEED      );
pub const SNDCTL_DSP_GETBLKSIZE:  c_ulong = iowr::<c_int>(DSP, CMD_GETBLKSIZE ); // ! obselete
pub const SNDCTL_DSP_SETFMT:      c_ulong = iowr::<c_int>(DSP, CMD_SETFMT     );
pub const SNDCTL_DSP_CHANNELS:    c_ulong = iowr::<c_int>(DSP, CMD_CHANNELS   );
pub const SNDCTL_DSP_SETFRAGMENT: c_ulong = iowr::<c_int>(DSP, CMD_SETFRAGMENT);
pub const SNDCTL_DSP_GETFMTS:     c_ulong = ior ::<c_int>(DSP, CMD_GETFMTS    );
pub const SNDCTL_DSP_GETCAPS:     c_ulong = ior ::<c_int>(DSP, CMD_GETCAPS    );
pub const SNDCTL_DSP_GETTRIGGER:  c_ulong = ior ::<c_int>(DSP, CMD_TRIGGER    );
pub const SNDCTL_DSP_SETTRIGGER:  c_ulong = iow ::<c_int>(DSP, CMD_TRIGGER    );
pub const SNDCTL_DSP_GETODELAY:   c_ulong = ior ::<c_int>(DSP, CMD_GETODELAY  );

pub const SNDCTL_DSP_GETOSPACE: c_ulong = ior::<AudioBufInfo>(DSP, CMD_GETOSPACE);
pub const SNDCTL_DSP_GETISPACE: c_ulong = ior::<AudioBufInfo>(DSP, CMD_GETISPACE);

// Sample Formats
// AFMT = Audio Format
pub const AFMT_QUERY:  c_int = 0;
pub const AFMT_U8:     c_int = 1 << 3;
pub const AFMT_S16_LE: c_int = 1 << 4;
pub const AFMT_S32_LE: c_int = 1 << 12;
pub const AFMT_FLOAT:  c_int = 1 << 14;

// Capability Flags
pub const PCM_CAP_DUPLEX:  c_int = 1 << 8;
pub const PCM_CAP_TRIGGER: c_int = 1 << 12;
pub const PCM_CAP_INPUT:   c_int = 1 << 16;
pub const PCM_CAP_OUTPUT:  c_int = 1 << 17;

/// FFI Mirror of OSS' `audio_buf_info`
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct AudioBufInfo {  // | Output:          | Input:                  |
    pub fragments:  c_int, // | Fragments free   | Fragments ready to read |
    pub fragstotal: c_int, // |      Total fragments in the buffer         |
    pub fragsize:   c_int, // |           bytes per fragment               |
    pub bytes:      c_int, // | Bytes free       | Bytes ready to read     |
}

// Compile time assertions
const _: () = assert!(SNDCTL_DSP_GETCAPS    == 0x4004_500F);
const _: () = assert!(SNDCTL_DSP_SPEED      == 0xC004_5002);
const _: () = assert!(SNDCTL_DSP_SETTRIGGER == 0x8004_5010);

/// [TODO]: cpal error returns?
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
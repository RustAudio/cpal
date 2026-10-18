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
const CMD_GETBLKSIZE:  u8 = 4;
const CMD_SETFMT:      u8 = 5;
const CMD_CHANNELS:    u8 = 6;
const CMD_SETFRAGMENT: u8 = 10;
const CMD_GETFMTS:     u8 = 11;
const CMD_GETCAPS:     u8 = 15;
const CMD_TRIGGER:     u8 = 16; // `GETTRIGGER` and `SETTRIGGER` share it, just with opposite directions
const CMD_GETODELAY:   u8 = 23;

pub const SNDCTL_DSP_SPEED:       c_ulong = iowr::<c_int>(DSP, CMD_SPEED      );
pub const SNDCTL_DSP_GETBLKSIZE:  c_ulong = iowr::<c_int>(DSP, CMD_GETBLKSIZE );
pub const SNDCTL_DSP_SETFMT:      c_ulong = iowr::<c_int>(DSP, CMD_SETFMT     );
pub const SNDCTL_DSP_CHANNELS:    c_ulong = iowr::<c_int>(DSP, CMD_CHANNELS   );
pub const SNDCTL_DSP_SETFRAGMENT: c_ulong = iowr::<c_int>(DSP, CMD_SETFRAGMENT);
pub const SNDCTL_DSP_GETFMTS:     c_ulong = ior ::<c_int>(DSP, CMD_GETFMTS    );
pub const SNDCTL_DSP_GETCAPS:     c_ulong = ior ::<c_int>(DSP, CMD_GETCAPS    );
pub const SNDCTL_DSP_GETTRIGGER:  c_ulong = ior ::<c_int>(DSP, CMD_TRIGGER    );
pub const SNDCTL_DSP_SETTRIGGER:  c_ulong = iow ::<c_int>(DSP, CMD_TRIGGER    );
pub const SNDCTL_DSP_GETODELAY:   c_ulong = ior ::<c_int>(DSP, CMD_GETODELAY  );

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

// Compile time assertions
const _: () = assert!(SNDCTL_DSP_GETCAPS    == 0x4004_500F);
const _: () = assert!(SNDCTL_DSP_SPEED      == 0xC004_5002);
const _: () = assert!(SNDCTL_DSP_SETTRIGGER == 0x8004_5010);
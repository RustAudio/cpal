//! Mapping between OSS `AFMT_*` values and `SampleFormat`.
//!
//! AFMT stands for Audio Format.
//!
//! The source of truth for much of this file is /usr/include/sys/soundcard.h from
//! FreeBSD-15.1-RELEASE

use libc::c_int;

use crate::SampleFormat;

// AFMT Sample Formats
//
// consts such as AFMT_FLOAT are excluded, as in <soundcard.h>, they are
// just set to AFMT_F32_NE, as it's an old compatibility thing
//
// AFMT_FULLDUPLEX is ignored, as its legacy and from before `GET_CAPS`,
// thus I don't want to use it even as a fallback to when `GET_CAPS` fails
// AFMT_QUERY = 0; // odd
// AFMT_MU_LAW     = 1 << 0;
// AFMT_A_LAW      = 1 << 1;
// AFMT_IMA_ADPCM  = 1 << 2;
pub const AFMT_U8: c_int = 1 << 3;
// AFMT_S16_LE     = 1 << 4;
// AFMT_S16_BE     = 1 << 5;
pub const AFMT_S8: c_int = 1 << 6;
// AFMT_U16_LE     = 1 << 7;
// AFMT_U16_BE     = 1 << 8;
// AFMT_MPEG       = 1 << 9;
// AFMT_AC3        = 1 << 10;
// AFMT_VORBIS     = 1 << 11;
// AFMT_S32_LE     = 1 << 12;
// AFMT_S32_BE     = 1 << 13;
// AFMT_S24_LE     = 1 << 14;
// AFMT_S24_BE     = 1 << 15;
// AFMT_S24_PACKED = 1 << 16;
// bits 17->27: unused and reserved
// bit 17 is a bit odd though, it seems to vary
// AFMT_F32_LE     = 1 << 28;
// AFMT_F32_BE     = 1 << 29;
// bit 30: `AFMT_FULLDUPLEX` (see above)
// bit 31: sign

// Native-endian aliases.
//
// Terminology:
// `_LE` Little Endian
// `_BE` Big Endian
// `_NE` Native Endian
// `_OE` Opposite Endian
#[cfg(target_endian = "little")]
mod ne_aliases {
    use libc::c_int;
    pub const AFMT_S16_NE: c_int = 1 << 4;
    pub const AFMT_U16_NE: c_int = 1 << 7;
    pub const AFMT_S32_NE: c_int = 1 << 12;
    pub const AFMT_S24_NE: c_int = 1 << 14;
    pub const AFMT_F32_NE: c_int = 1 << 28;
}

#[cfg(target_endian = "big")]
mod ne_aliases {
    use libc::c_int;
    pub const AFMT_S16_NE: c_int = 1 << 5;
    pub const AFMT_U16_NE: c_int = 1 << 8;
    pub const AFMT_S32_NE: c_int = 1 << 13;
    pub const AFMT_S24_NE: c_int = 1 << 15;
    pub const AFMT_F32_NE: c_int = 1 << 29;
}

pub use ne_aliases::*;

/// Contains the subset of `SampleFormat` variants that have an OSS
/// `AFMT_*` equivalent.
pub(super) const FORMATS: &[(c_int, SampleFormat)] = &[
    (AFMT_F32_NE, SampleFormat::F32),
    (AFMT_S32_NE, SampleFormat::I32),
    (AFMT_S24_NE, SampleFormat::I24),
    (AFMT_S16_NE, SampleFormat::I16),
    (AFMT_U16_NE, SampleFormat::U16),
    (AFMT_U8, SampleFormat::U8),
    (AFMT_S8, SampleFormat::I8),
];

/// Convert a `SampleFormat` to its OSS `AFMT_*` constant (if possible).
pub(super) fn afmt_from_sample_format(format: SampleFormat) -> Option<c_int> {
    match format {
        SampleFormat::F32 => Some(AFMT_F32_NE),
        SampleFormat::I32 => Some(AFMT_S32_NE),
        SampleFormat::I24 => Some(AFMT_S24_NE),
        SampleFormat::I16 => Some(AFMT_S16_NE),
        SampleFormat::U16 => Some(AFMT_U16_NE),
        SampleFormat::U8 => Some(AFMT_U8),
        SampleFormat::I8 => Some(AFMT_S8),
        _ => None,
    }
}

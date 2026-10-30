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
pub const AFMT_QUERY: c_int = 0; // odd
pub const AFMT_MU_LAW: c_int = 1 << 0;
pub const AFMT_A_LAW: c_int = 1 << 1;
pub const AFMT_IMA_ADPCM: c_int = 1 << 2;
pub const AFMT_U8: c_int = 1 << 3;
pub const AFMT_S16_LE: c_int = 1 << 4;
pub const AFMT_S16_BE: c_int = 1 << 5;
pub const AFMT_S8: c_int = 1 << 6;
pub const AFMT_U16_LE: c_int = 1 << 7;
pub const AFMT_U16_BE: c_int = 1 << 8;
pub const AFMT_MPEG: c_int = 1 << 9;
pub const AFMT_AC3: c_int = 1 << 10;
pub const AFMT_VORBIS: c_int = 1 << 11;
pub const AFMT_S32_LE: c_int = 1 << 12;
pub const AFMT_S32_BE: c_int = 1 << 13;
pub const AFMT_S24_LE: c_int = 1 << 14;
pub const AFMT_S24_BE: c_int = 1 << 15;
pub const AFMT_S24_PACKED: c_int = 1 << 16;
// bits 17->27: unused and reserved
// bit 17 is a bit odd though, it seems to vary
pub const AFMT_F32_LE: c_int = 1 << 28;
pub const AFMT_F32_BE: c_int = 1 << 29;
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
    use super::*;
    pub const AFMT_S16_NE: c_int = AFMT_S16_LE;
    pub const AFMT_U16_NE: c_int = AFMT_U16_LE;
    pub const AFMT_S32_NE: c_int = AFMT_S32_LE;
    pub const AFMT_S24_NE: c_int = AFMT_S24_LE;
    pub const AFMT_F32_NE: c_int = AFMT_F32_LE;
}

#[cfg(target_endian = "big")]
mod ne_aliases {
    use super::*;
    pub const AFMT_S16_NE: c_int = AFMT_S16_BE;
    pub const AFMT_U16_NE: c_int = AFMT_U16_BE;
    pub const AFMT_S32_NE: c_int = AFMT_S32_BE;
    pub const AFMT_S24_NE: c_int = AFMT_S24_BE;
    pub const AFMT_F32_NE: c_int = AFMT_F32_BE;
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

pub(super) fn sample_format_from_afmt(afmt: c_int) -> Option<SampleFormat> {
    FORMATS.iter().find(|&&(a, _)| a == afmt).map(|&(_, f)| f)
}

/// Convert a `SampleFormat` to its OSS `AFMT_*` constant (if possible).
// [TODO]: consider returning a Result<(), crate::Error> with UnsupportedConfig
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for &(afmt, fmt) in FORMATS {
            assert_eq!(sample_format_from_afmt(afmt), Some(fmt));
            assert_eq!(afmt_from_sample_format(fmt), Some(afmt));
        }
    }

    #[test]
    fn formats_without_afmt_are_none() {
        assert_eq!(afmt_from_sample_format(SampleFormat::I64), None);
        assert_eq!(afmt_from_sample_format(SampleFormat::F64), None);
        assert_eq!(afmt_from_sample_format(SampleFormat::DsdU8), None);
    }

    #[test]
    fn unknown_afmt_is_none() {
        assert_eq!(sample_format_from_afmt(-1), None);
    }
}

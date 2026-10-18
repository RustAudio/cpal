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
// just set to AFMT_F32_NE, as it's an old compatability thing
//
// AFMT_FULLDUPLEX is ignored, as its legacy and from before `GET_CAPS`,
// thus I don't want to use it even as a fallback to when `GET_CAPS` fails
pub const AFMT_QUERY:     c_int = 0; // odd
pub const AFMT_MU_LAW:    c_int = 1 << 0;
pub const AFMT_A_LAW:     c_int = 1 << 1;
pub const AFMT_IMA_ADPCM: c_int = 1 << 2;
pub const AFMT_U8:        c_int = 1 << 3;
pub const AFMT_S16_LE:    c_int = 1 << 4;
pub const AFMT_S16_BE:    c_int = 1 << 5;
pub const AFMT_S8:        c_int = 1 << 6;
pub const AFMT_U16_LE:    c_int = 0x00000080;
pub const AFMT_U16_BE:    c_int = 0x00000100;
pub const AFMT_MPEG:      c_int = 0x00000200;
pub const AFMT_S32_LE:    c_int = 1 << 12;
pub const AFMT_F32_LE:    c_int = 0x10000000;
pub const AFMT_F32_BE:    c_int = 0x20000000;

#[cfg(target_endian = "little")]
pub const AFMT_F32_NE: c_int = AFMT_F32_LE;
#[cfg(target_endian = "big")]
pub const AFMT_F32_NE: c_int = AFMT_F32_BE;

pub(super) const FORMATS: &[(c_int, SampleFormat)] = &[
    (AFMT_F32_NE, SampleFormat::F32),
    (AFMT_S32_LE, SampleFormat::I32),
    (AFMT_S16_LE, SampleFormat::I16),
    (AFMT_U8,     SampleFormat::U8 ),
];

pub(super) fn sample_format_from_afmt(afmt: c_int) -> Option<SampleFormat> {
    FORMATS.iter().find(|&&(a, _)| a == afmt).map(|&(_, f)| f)
}

// [TODO] make infalliable
pub(super) fn afmt_from_sample_format(format: SampleFormat) -> Option<c_int> {
    FORMATS.iter().find(|&&(_, f)| f == format).map(|&(a, _)| a)
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
    fn unknown_afmt_is_none() {
        assert_eq!(sample_format_from_afmt(-1), None);
    }
}
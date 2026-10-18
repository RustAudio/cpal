//! Mapping between OSS `AFMT_*` values and `SampleFormat`.

use libc::c_int;

use super::syscall::{AFMT_FLOAT, AFMT_S16_LE, AFMT_S32_LE, AFMT_U8};
use crate::SampleFormat;

pub(super) const FORMATS: &[(c_int, SampleFormat)] = &[
    (AFMT_FLOAT,  SampleFormat::F32),
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
use crate::{Sample, SampleFormat, U24};

pub const DSD_EQUILIBRIUM_BYTE: u8 = 0x69;
pub const U8_EQUILIBRIUM_BYTE: u8 = 0x80;

/// Fill `buffer` with the equilibrium value for any `sample_format`.
#[inline]
pub fn fill_equilibrium(buffer: &mut [u8], sample_format: SampleFormat) {
    macro_rules! fill_typed {
        ($sample_type:ty) => {{
            let sample_size = std::mem::size_of::<$sample_type>();

            debug_assert_eq!(
                buffer.len() % sample_size,
                0,
                "Buffer size must be aligned to sample size for format {:?}",
                sample_format
            );

            let num_samples = buffer.len() / sample_size;
            let equilibrium = <$sample_type as Sample>::EQUILIBRIUM;

            // Safety: buffer length is verified to be a multiple of the sample size above.
            let samples = unsafe {
                std::slice::from_raw_parts_mut(
                    buffer.as_mut_ptr() as *mut $sample_type,
                    num_samples,
                )
            };

            for sample in samples {
                *sample = equilibrium;
            }
        }};
    }

    if sample_format.is_int() || sample_format.is_float() {
        buffer.fill(0);
    } else if sample_format == SampleFormat::U8 {
        buffer.fill(U8_EQUILIBRIUM_BYTE);
    } else if sample_format.is_dsd() {
        buffer.fill(DSD_EQUILIBRIUM_BYTE);
    } else {
        // Multi-byte unsigned integer formats require a fill equal to the midpoint of their range.
        debug_assert!(sample_format.is_uint());
        match sample_format {
            SampleFormat::U16 => fill_typed!(u16),
            SampleFormat::U24 => fill_typed!(U24),
            SampleFormat::U32 => fill_typed!(u32),
            SampleFormat::U64 => fill_typed!(u64),
            _ => unimplemented!(
                "failed to fill equilibrium for unsupported unsigned format {sample_format:?}"
            ),
        }
    }
}

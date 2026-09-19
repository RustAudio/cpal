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

#[test]
fn test_fill_equilibrium_byte_patterns() {
    let mut buf = vec![0u8; 8].into_boxed_slice();

    // Unsigned 8-bit silence is 0x80, the midpoint of the range.
    fill_equilibrium(&mut buf[..], SampleFormat::U8);
    assert_eq!(buf[0], 0x80);
    assert_eq!(buf[5], 0x80);

    // Signed and float formats rest at zero.
    fill_equilibrium(&mut buf[..], SampleFormat::I16);
    assert_eq!(buf[0], 0);
    assert_eq!(buf[5], 0);
    fill_equilibrium(&mut buf[..], SampleFormat::I24);
    assert_eq!(buf[0], 0);
    assert_eq!(buf[5], 0);
    fill_equilibrium(&mut buf[..], SampleFormat::F32);
    assert_eq!(buf[0], 0);
    assert_eq!(buf[5], 0);

    // DSD silence is the 0x69 pattern.
    fill_equilibrium(&mut buf[..], SampleFormat::DsdU8);
    assert_eq!(buf[0], 0x69);
    assert_eq!(buf[5], 0x69);

    // Multi-byte unsigned formats take the typed path, the only one that casts the buffer to a
    // wider pointer: check the value written and that `chunks_exact` accounts for every byte.
    fill_equilibrium(&mut buf[..], SampleFormat::U16);
    assert!(
        buf.chunks_exact(2)
            .all(|c| u16::from_ne_bytes(c.try_into().unwrap()) == 0x8000)
    );
    fill_equilibrium(&mut buf[..], SampleFormat::U24);
    assert!(
        buf.chunks_exact(4)
            .all(|c| i32::from_ne_bytes(c.try_into().unwrap()) == 0x0080_0000)
    );
    fill_equilibrium(&mut buf[..], SampleFormat::U32);
    assert!(
        buf.chunks_exact(4)
            .all(|c| u32::from_ne_bytes(c.try_into().unwrap()) == 0x8000_0000)
    );
    fill_equilibrium(&mut buf[..], SampleFormat::U64);
    assert!(
        buf.chunks_exact(8)
            .all(|c| u64::from_ne_bytes(c.try_into().unwrap()) == 0x8000_0000_0000_0000)
    );
}

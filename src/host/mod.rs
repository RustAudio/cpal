use crate::{Sample, SampleFormat, I24, U24};

#[cfg(target_os = "android")]
pub(crate) mod aaudio;
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
))]
pub(crate) mod alsa;
#[cfg(all(windows, feature = "asio"))]
pub(crate) mod asio;
#[cfg(all(
    feature = "wasm-bindgen",
    feature = "audioworklet",
    target_feature = "atomics"
))]
pub(crate) mod audioworklet;
#[cfg(windows)]
pub(crate) mod com;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod coreaudio;
#[cfg(target_os = "emscripten")]
pub(crate) mod emscripten;
#[cfg(all(
    feature = "jack",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "macos",
        target_os = "windows",
    )
))]
pub(crate) mod jack;
#[cfg(all(
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
    ),
    feature = "pipewire"
))]
pub(crate) mod pipewire;
#[cfg(all(
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ),
    feature = "pulseaudio"
))]
pub(crate) mod pulseaudio;
#[cfg(windows)]
pub(crate) mod wasapi;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub(crate) mod webaudio;

#[cfg(feature = "custom")]
pub(crate) mod custom;
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "macos",
    target_os = "ios",
    target_os = "emscripten",
    target_os = "android",
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
)))]
pub(crate) mod null;

// Fill a buffer with equilibrium values for any sample format.
// Works with any buffer size, even if not perfectly aligned to sample boundaries.
#[allow(unused)]
pub(crate) fn fill_with_equilibrium(buffer: &mut [u8], sample_format: SampleFormat) {
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

            // Safety: We verified the buffer size is correctly aligned for the sample type
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
    const DSD_SILENCE_BYTE: u8 = 0x69;

    match sample_format {
        SampleFormat::I8 => fill_typed!(i8),
        SampleFormat::I16 => fill_typed!(i16),
        SampleFormat::I24 => fill_typed!(I24),
        SampleFormat::I32 => fill_typed!(i32),
        // SampleFormat::I48 => fill_typed!(I48),
        SampleFormat::I64 => fill_typed!(i64),
        SampleFormat::U8 => fill_typed!(u8),
        SampleFormat::U16 => fill_typed!(u16),
        SampleFormat::U24 => fill_typed!(U24),
        SampleFormat::U32 => fill_typed!(u32),
        // SampleFormat::U48 => fill_typed!(U48),
        SampleFormat::U64 => fill_typed!(u64),
        SampleFormat::F32 => fill_typed!(f32),
        SampleFormat::F64 => fill_typed!(f64),
        SampleFormat::DsdU8 | SampleFormat::DsdU16 | SampleFormat::DsdU32 => {
            buffer.fill(DSD_SILENCE_BYTE)
        }
    }
}

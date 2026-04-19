#[cfg(any(
    target_os = "linux",
    target_os = "windows",
    target_vendor = "apple",
    feature = "audioworklet",
    all(
        feature = "jack",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows",
        )
    )
))]
use crate::{FrameCount, SampleRate};

#[cfg(any(
    target_os = "linux",
    target_os = "windows",
    target_vendor = "apple",
    feature = "audioworklet",
    all(
        feature = "jack",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows",
        )
    )
))]
use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "windows"))]
use crate::{Sample, SampleFormat, U24};

#[cfg(windows)]
pub(crate) mod com;

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

#[cfg(target_vendor = "apple")]
pub(crate) mod coreaudio;

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
    target_vendor = "apple",
    target_os = "android",
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
)))]
pub(crate) mod null;

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) const DSD_EQUILIBRIUM_BYTE: u8 = 0x69;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) const U8_EQUILIBRIUM_BYTE: u8 = 0x80;

/// Fill `buffer` with the equilibrium value for any `sample_format`.
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[inline]
pub(crate) fn fill_equilibrium(buffer: &mut [u8], sample_format: SampleFormat) {
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

/// Convert a frame count at a given sample rate to a [`std::time::Duration`].
#[cfg(any(
    target_os = "linux",
    target_os = "windows",
    target_vendor = "apple",
    feature = "audioworklet",
    all(
        feature = "jack",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows",
        )
    )
))]
#[inline]
pub(crate) fn frames_to_duration(frames: FrameCount, rate: SampleRate) -> Duration {
    if rate == 0 {
        return Duration::ZERO;
    }
    let rate = rate as u64;
    let secs = frames as u64 / rate;
    // rem_frames < rate <= u32::MAX, so rem_frames * 1_000_000_000 < u64::MAX
    let rem_frames = frames as u64 % rate;
    let nanos = rem_frames * 1_000_000_000 / rate;
    Duration::new(secs, nanos as u32)
}

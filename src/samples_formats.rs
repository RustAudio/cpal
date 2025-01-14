use std::{fmt::Display, mem};
#[cfg(target_os = "emscripten")]
use wasm_bindgen::prelude::*;

pub use dasp_sample::{FromSample, Sample, I24, I48, U24, U48};

/// Format that each sample has. Usually, this corresponds to the sampling
/// depth of the audio source. For example, 16 bit quantized samples can be
/// encoded in `i16` or `u16`. Note that the sampling depth is not directly
/// visible for formats where [`is_float`] is true.
///
/// Also note that the backend must support the encoding of the quantized
/// samples in the given format, as there is no generic transformation from one
/// format into the other done inside the frontend-library code. You can query
/// the supported formats by using [`supported_input_configs`].
///
/// A good rule of thumb is to use [`SampleFormat::I16`] as this covers typical
/// music (WAV, MP3) as well as typical audio input devices on most platforms,
///
/// [`is_float`]: SampleFormat::is_float
/// [`supported_input_configs`]: crate::Device::supported_input_configs
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SampleFormat {
    /// `i8` with a valid range of `i8::MIN..=i8::MAX` with `0` being the origin.
    I8,

    /// `i16` with a valid range of `i16::MIN..=i16::MAX` with `0` being the origin.
    I16,

    /// `I24` with a valid range of '-(1 << 23)..(1 << 23)' with `0` being the origin
    I24,

    /// `i32` with a valid range of `i32::MIN..=i32::MAX` with `0` being the origin.
    I32,

    // /// `I48` with a valid range of '-(1 << 47)..(1 << 47)' with `0` being the origin
    // I48,
    /// `i64` with a valid range of `i64::MIN..=i64::MAX` with `0` being the origin.
    I64,

    /// `u8` with a valid range of `u8::MIN..=u8::MAX` with `1 << 7 == 128` being the origin.
    U8,

    /// `u16` with a valid range of `u16::MIN..=u16::MAX` with `1 << 15 == 32768` being the origin.
    U16,

    /// `U24` with a valid range of '0..16777216' with `1 << 23 == 8388608` being the origin
    // U24,

    /// `u32` with a valid range of `u32::MIN..=u32::MAX` with `1 << 31` being the origin.
    U32,

    /// `U48` with a valid range of '0..(1 << 48)' with `1 << 47` being the origin
    // U48,

    /// `u64` with a valid range of `u64::MIN..=u64::MAX` with `1 << 63` being the origin.
    U64,

    /// `f32` with a valid range of `-1.0..1.0` with `0.0` being the origin.
    F32,

    /// `f64` with a valid range of `-1.0..1.0` with `0.0` being the origin.
    F64,
}

impl SampleFormat {
    /// Returns the size in bytes of a sample of this format. This corresponds to
    /// the internal size of the rust primitives that are used to represent this
    /// sample format (e.g., i24 has size of i32).
    #[inline]
    #[must_use]
    pub fn sample_size(&self) -> usize {
        match *self {
            SampleFormat::I8 | SampleFormat::U8 => mem::size_of::<i8>(),
            SampleFormat::I16 | SampleFormat::U16 => mem::size_of::<i16>(),
            SampleFormat::I24 => mem::size_of::<i32>(),
            // SampleFormat::U24 => mem::size_of::<i32>(),
            SampleFormat::I32 | SampleFormat::U32 => mem::size_of::<i32>(),

            // SampleFormat::I48 => mem::size_of::<i64>(),
            // SampleFormat::U48 => mem::size_of::<i64>(),
            SampleFormat::I64 | SampleFormat::U64 => mem::size_of::<i64>(),
            SampleFormat::F32 => mem::size_of::<f32>(),
            SampleFormat::F64 => mem::size_of::<f64>(),
        }
    }

    #[inline]
    #[must_use]
    pub fn is_int(&self) -> bool {
        matches!(
            *self,
            SampleFormat::I8
                | SampleFormat::I16
                | SampleFormat::I24
                | SampleFormat::I32
                // | SampleFormat::I48
                | SampleFormat::I64
        )
    }

    #[inline]
    #[must_use]
    pub fn is_uint(&self) -> bool {
        matches!(
            *self,
            SampleFormat::U8
                | SampleFormat::U16
                // | SampleFormat::U24
                | SampleFormat::U32
                // | SampleFormat::U48
                | SampleFormat::U64
        )
    }

    #[inline]
    #[must_use]
    pub fn is_float(&self) -> bool {
        matches!(*self, SampleFormat::F32 | SampleFormat::F64)
    }
}

impl Display for SampleFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            SampleFormat::I8 => "i8",
            SampleFormat::I16 => "i16",
            SampleFormat::I24 => "i24",
            SampleFormat::I32 => "i32",
            // SampleFormat::I48 => "i48",
            SampleFormat::I64 => "i64",
            SampleFormat::U8 => "u8",
            SampleFormat::U16 => "u16",
            // SampleFormat::U24 => "u24",
            SampleFormat::U32 => "u32",
            // SampleFormat::U48 => "u48",
            SampleFormat::U64 => "u64",
            SampleFormat::F32 => "f32",
            SampleFormat::F64 => "f64",
        }
        .fmt(f)
    }
}

pub trait SizedSample: Sample {
    const FORMAT: SampleFormat;
}

impl SizedSample for i8 {
    const FORMAT: SampleFormat = SampleFormat::I8;
}

impl SizedSample for i16 {
    const FORMAT: SampleFormat = SampleFormat::I16;
}

impl SizedSample for I24 {
    const FORMAT: SampleFormat = SampleFormat::I24;
}

impl SizedSample for i32 {
    const FORMAT: SampleFormat = SampleFormat::I32;
}

// impl SizedSample for I48 {
//     const FORMAT: SampleFormat = SampleFormat::I48;
// }

impl SizedSample for i64 {
    const FORMAT: SampleFormat = SampleFormat::I64;
}

impl SizedSample for u8 {
    const FORMAT: SampleFormat = SampleFormat::U8;
}

impl SizedSample for u16 {
    const FORMAT: SampleFormat = SampleFormat::U16;
}

// impl SizedSample for U24 {
//     const FORMAT: SampleFormat = SampleFormat::U24;
// }

impl SizedSample for u32 {
    const FORMAT: SampleFormat = SampleFormat::U32;
}

// impl SizedSample for U48 {
//     const FORMAT: SampleFormat = SampleFormat::U48;
// }

impl SizedSample for u64 {
    const FORMAT: SampleFormat = SampleFormat::U64;
}

impl SizedSample for f32 {
    const FORMAT: SampleFormat = SampleFormat::F32;
}

impl SizedSample for f64 {
    const FORMAT: SampleFormat = SampleFormat::F64;
}

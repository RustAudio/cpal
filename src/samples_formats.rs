use std::{fmt::Display, mem};
#[cfg(target_os = "emscripten")]
use wasm_bindgen::prelude::*;

pub use dasp_sample::{FromSample, Sample, I24, I48, U24, U48};

/// Format that each sample has.
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SampleFormat {
    /// `i8` with a valid range of 'u8::MIN..=u8::MAX' with `0` being the origin
    I8,

    /// `i16` with a valid range of 'u16::MIN..=u16::MAX' with `0` being the origin
    I16,

    // /// `I24` with a valid range of '-(1 << 23)..(1 << 23)' with `0` being the origin
    // I24,
    /// `i32` with a valid range of 'u32::MIN..=u32::MAX' with `0` being the origin
    I32,

    // /// `I24` with a valid range of '-(1 << 47)..(1 << 47)' with `0` being the origin
    // I48,
    /// `i64` with a valid range of 'u64::MIN..=u64::MAX' with `0` being the origin
    I64,

    /// `u8` with a valid range of 'u8::MIN..=u8::MAX' with `1 << 7 == 128` being the origin
    U8,

    /// `u16` with a valid range of 'u16::MIN..=u16::MAX' with `1 << 15 == 32768` being the origin
    U16,

    // /// `U24` with a valid range of '0..16777216' with `1 << 23 == 8388608` being the origin
    // U24,
    /// `u32` with a valid range of 'u32::MIN..=u32::MAX' with `1 << 31` being the origin
    U32,

    // /// `U48` with a valid range of '0..(1 << 48)' with `1 << 47` being the origin
    // U48,
    /// `u64` with a valid range of 'u64::MIN..=u64::MAX' with `1 << 63` being the origin
    U64,

    /// `f32` with a valid range of `-1.0..1.0` with `0.0` being the origin
    F32,

    /// `f64` with a valid range of -1.0..1.0 with 0.0 being the origin
    F64,
}

impl SampleFormat {
    /// Returns the size in bytes of a sample of this format.
    #[inline]
    #[must_use]
    pub fn sample_size(&self) -> usize {
        match *self {
            SampleFormat::I8 | SampleFormat::U8 => mem::size_of::<i8>(),
            SampleFormat::I16 | SampleFormat::U16 => mem::size_of::<i16>(),
            // SampleFormat::I24 | SampleFormat::U24 => 3,
            SampleFormat::I32 | SampleFormat::U32 => mem::size_of::<i32>(),
            // SampleFormat::I48 | SampleFormat::U48 => 6,
            SampleFormat::I64 | SampleFormat::U64 => mem::size_of::<i64>(),
            SampleFormat::F32 => mem::size_of::<f32>(),
            SampleFormat::F64 => mem::size_of::<f64>(),
        }
    }

    #[inline]
    #[must_use]
    pub fn is_int(&self) -> bool {
        //matches!(*self, SampleFormat::I8 | SampleFormat::I16 | SampleFormat::I24 | SampleFormat::I32 | SampleFormat::I48 | SampleFormat::I64)
        matches!(
            *self,
            SampleFormat::I8 | SampleFormat::I16 | SampleFormat::I32 | SampleFormat::I64
        )
    }

    #[inline]
    #[must_use]
    pub fn is_uint(&self) -> bool {
        //matches!(*self, SampleFormat::U8 | SampleFormat::U16 | SampleFormat::U24 | SampleFormat::U32 | SampleFormat::U48 | SampleFormat::U64)
        matches!(
            *self,
            SampleFormat::U8 | SampleFormat::U16 | SampleFormat::U32 | SampleFormat::U64
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
            // SampleFormat::I24 => "i24",
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

// impl SizedSample for I24 { const FORMAT: SampleFormat = SampleFormat::I24; }

impl SizedSample for i32 {
    const FORMAT: SampleFormat = SampleFormat::I32;
}

// impl SizedSample for I48 { const FORMAT: SampleFormat = SampleFormat::I48; }

impl SizedSample for i64 {
    const FORMAT: SampleFormat = SampleFormat::I64;
}

impl SizedSample for u8 {
    const FORMAT: SampleFormat = SampleFormat::U8;
}

impl SizedSample for u16 {
    const FORMAT: SampleFormat = SampleFormat::U16;
}

// impl SizedSample for U24 { const FORMAT: SampleFormat = SampleFormat::U24; }

impl SizedSample for u32 {
    const FORMAT: SampleFormat = SampleFormat::U32;
}

// impl SizedSample for U48 { const FORMAT: SampleFormat = SampleFormat::U48; }

impl SizedSample for u64 {
    const FORMAT: SampleFormat = SampleFormat::U64;
}

impl SizedSample for f32 {
    const FORMAT: SampleFormat = SampleFormat::F32;
}

impl SizedSample for f64 {
    const FORMAT: SampleFormat = SampleFormat::F64;
}

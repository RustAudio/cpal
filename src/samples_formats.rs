use std::mem;

/// Format that each sample has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// The value 0 corresponds to 0.
    I16,
    /// The value 0 corresponds to 32768.
    U16,
    /// The boundaries are (-1.0, 1.0).
    F32,
}

impl SampleFormat {
    /// Returns the size in bytes of a sample of this format.
    #[inline]
    pub fn sample_size(&self) -> usize {
        match self {
            &SampleFormat::I16 => mem::size_of::<i16>(),
            &SampleFormat::U16 => mem::size_of::<u16>(),
            &SampleFormat::F32 => mem::size_of::<f32>(),
        }
    }
}

/// Trait for containers that contain PCM data.
pub unsafe trait Sample: Copy + Clone {
    /// Returns the `SampleFormat` corresponding to this data type.
    fn format() -> SampleFormat;
}

unsafe impl Sample for u16 {
    #[inline]
    fn format() -> SampleFormat {
        SampleFormat::U16
    }
}

unsafe impl Sample for i16 {
    #[inline]
    fn format() -> SampleFormat {
        SampleFormat::I16
    }
}

unsafe impl Sample for f32 {
    #[inline]
    fn format() -> SampleFormat {
        SampleFormat::F32
    }
}

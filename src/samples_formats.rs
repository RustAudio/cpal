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

    /// Deprecated. Use `sample_size` instead.
    #[inline]
    #[deprecated]
    pub fn get_sample_size(&self) -> usize {
        self.sample_size()
    }
}

/// Trait for containers that contain PCM data.
pub unsafe trait Sample: Copy + Clone {
    /// Returns the `SampleFormat` corresponding to this data type.
    // TODO: rename to `format()`. Requires a breaking change.
    fn get_format() -> SampleFormat;

    /// Turns the sample into its equivalent as a floating-point.
    fn to_f32(&self) -> f32;
}

unsafe impl Sample for u16 {
    #[inline]
    fn get_format() -> SampleFormat {
        SampleFormat::U16
    }

    #[inline]
    fn to_f32(&self) -> f32 {
        ((*self as f32 / u16::max_value() as f32) - 0.5) * 2.0
    }
}

unsafe impl Sample for i16 {
    #[inline]
    fn get_format() -> SampleFormat {
        SampleFormat::I16
    }

    #[inline]
    fn to_f32(&self) -> f32 {
        (*self as f32 / i16::max_value() as f32) + 0.5
    }
}

unsafe impl Sample for f32 {
    #[inline]
    fn get_format() -> SampleFormat {
        SampleFormat::F32
    }

    #[inline]
    fn to_f32(&self) -> f32 {
        *self
    }
}

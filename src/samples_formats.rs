use std::mem;

/// Format that each sample has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// The value 0 corresponds to 0.
    I16,
    I32,
    /// The value 0 corresponds to 32768.
    U16,
    /// The boundaries are (-1.0, 1.0).
    F32,
}

impl SampleFormat {
    /// Returns the size in bytes of a sample of this format.
    #[inline]
    pub fn sample_size(&self) -> usize {
        match *self {
            SampleFormat::I16 => mem::size_of::<i16>(),
            SampleFormat::U16 => mem::size_of::<u16>(),
            SampleFormat::F32 => mem::size_of::<f32>(),
            SampleFormat::I32 => mem::size_of::<i32>(),
        }
    }
}

/// Trait for containers that contain PCM data.
pub unsafe trait Sample: Copy + Clone {
    /// The `SampleFormat` corresponding to this data type.
    const FORMAT: SampleFormat;

    /// Turns the sample into its equivalent as a floating-point.
    fn to_f32(&self) -> f32;
    /// Converts this sample into a standard u16 sample.
    fn to_u16(&self) -> u16;
    /// Converts this sample into a standard i16 sample.
    fn to_i16(&self) -> i16;
    /// Converts this sample into a standard i32 sample.
    fn to_i32(&self) -> i32;

    /// Converts any sample type to this one by calling `to_i16`, `to_i32`, `to_u16`,  or `to_f32`.
    fn from<S>(sample: &S) -> Self
    where
        S: Sample;
}

unsafe impl Sample for u16 {
    const FORMAT: SampleFormat = SampleFormat::U16;

    #[inline]
    fn to_f32(&self) -> f32 {
        self.to_i16().to_f32()
    }

    #[inline]
    fn to_u16(&self) -> u16 {
        *self
    }

    #[inline]
    fn to_i16(&self) -> i16 {
        if *self >= 32768 {
            (*self - 32768) as i16
        } else {
            (*self as i16) - 32767 - 1
        }
    }

    #[inline]
    fn to_i32(&self) -> i32 {
        self.to_f32().to_i32()
    }

    #[inline]
    fn from<S>(sample: &S) -> Self
    where
        S: Sample,
    {
        sample.to_u16()
    }
}

unsafe impl Sample for i16 {
    const FORMAT: SampleFormat = SampleFormat::I16;

    #[inline]
    fn to_f32(&self) -> f32 {
        if *self < 0 {
            *self as f32 / -(::std::i16::MIN as f32)
        } else {
            *self as f32 / ::std::i16::MAX as f32
        }
    }

    #[inline]
    fn to_u16(&self) -> u16 {
        if *self < 0 {
            (*self - ::std::i16::MIN) as u16
        } else {
            (*self as u16) + 32768
        }
    }

    #[inline]
    fn to_i16(&self) -> i16 {
        *self
    }

    #[inline]
    fn to_i32(&self) -> i32 {
        self.to_f32().to_i32()
    }

    #[inline]
    fn from<S>(sample: &S) -> Self
    where
        S: Sample,
    {
        sample.to_i16()
    }
}

unsafe impl Sample for f32 {
    const FORMAT: SampleFormat = SampleFormat::F32;

    #[inline]
    fn to_f32(&self) -> f32 {
        *self
    }

    /// This function inherently returns a lossy value due to scaling.
    #[inline]
    fn to_u16(&self) -> u16 {
        (((*self + 1.0) * 0.5) * ::std::u16::MAX as f32).round() as u16
    }

    /// This function inherently returns a lossy value due to scaling.
    #[inline]
    fn to_i16(&self) -> i16 {
        if *self >= 0.0 {
            (*self * ::std::i16::MAX as f32) as i16
        } else {
            (-*self * ::std::i16::MIN as f32) as i16
        }
    }

    #[inline]
    fn to_i32(&self) -> i32 {
        if self.is_sign_positive() {
            (*self as f64 * std::i32::MAX as f64).round() as i32
        } else {
            (*self as f64 * -(std::i32::MIN as f64)).round() as i32
        }
    }

    #[inline]
    fn from<S>(sample: &S) -> Self
    where
        S: Sample,
    {
        sample.to_f32()
    }
}

unsafe impl Sample for i32 {
    const FORMAT: SampleFormat = SampleFormat::I32;

    /// This function inherently returns a lossy value due to scaling.
    #[inline]
    fn to_f32(&self) -> f32 {
        if *self < 0 {
            (*self as f64 * (1.0 / -(::std::i32::MIN as f64))) as f32
        } else {
            (*self as f64 * (1.0 / ::std::i32::MAX as f64)) as f32
        }
    }

    /// This function inherently returns a lossy value due to scaling.
    #[inline]
    fn to_i16(&self) -> i16 {
        self.to_f32().to_i16()
    }

    /// This function inherently returns a lossy value due to scaling.
    #[inline]
    fn to_u16(&self) -> u16 {
        self.to_f32().to_u16()
    }

    #[inline]
    fn to_i32(&self) -> i32 {
        *self
    }

    #[inline]
    fn from<S>(sample: &S) -> Self
    where
        S: Sample,
    {
        sample.to_i32()
    }
}

#[cfg(test)]
mod test {
    use super::Sample;

    #[test]
    fn i32_to_i16() {
        assert_eq!(std::i32::MAX.to_i16(), std::i16::MAX);
        assert_eq!((std::i32::MIN / 2).to_i16(), std::i16::MIN / 2);
        assert_eq!(std::i32::MIN.to_i16(), std::i16::MIN);
        assert_eq!(0i32.to_i16(), 0);
    }

    #[test]
    fn i32_to_i32() {
        assert_eq!(std::i32::MAX.to_i32(), std::i32::MAX);
        assert_eq!((std::i32::MIN / 2).to_i32(), std::i32::MIN / 2);
        assert_eq!(std::i32::MIN.to_i32(), std::i32::MIN);
        assert_eq!(0i32.to_i32(), 0);
    }

    #[test]
    fn i32_to_u16() {
        assert_eq!(std::i32::MAX.to_u16(), std::u16::MAX);
        assert_eq!(0i32.to_u16(), (std::u16::MAX as f32 / 2.0).round() as u16);
        assert_eq!(std::i32::MIN.to_u16(), std::u16::MIN);
    }

    #[test]
    fn i32_to_f32() {
        assert_eq!(std::i32::MAX.to_f32(), 1.0f32);
        assert_eq!((std::i32::MAX / 8).to_f32(), 0.125f32);
        assert_eq!((std::i32::MAX / -16).to_f32(), -0.0625f32);
        assert_eq!((std::i32::MAX / -4).to_f32(), -0.25f32);
        assert_eq!(std::i32::MIN.to_f32(), -1.0f32);
        assert_eq!(0.to_f32(), 0f32);
    }

    #[test]
    fn i16_to_i16() {
        assert_eq!(0i16.to_i16(), 0);
        assert_eq!((-467i16).to_i16(), -467);
        assert_eq!(32767i16.to_i16(), 32767);
        assert_eq!((-32768i16).to_i16(), -32768);
    }

    #[test]
    fn i16_to_i32() {
        assert_eq!(0i16.to_i32(), 0);
        assert_eq!(std::i16::MAX.to_i32(), std::i32::MAX);
        assert_eq!(std::i16::MIN.to_i32(), std::i32::MIN);
    }

    #[test]
    fn i16_to_u16() {
        assert_eq!(0i16.to_u16(), 32768);
        assert_eq!((-16384i16).to_u16(), 16384);
        assert_eq!(32767i16.to_u16(), 65535);
        assert_eq!((-32768i16).to_u16(), 0);
    }

    #[test]
    fn i16_to_f32() {
        assert_eq!(0i16.to_f32(), 0.0);
        assert_eq!((-16384i16).to_f32(), -0.5);
        assert_eq!((-16384i16 / 2).to_f32(), -0.25);
        assert_eq!(32767i16.to_f32(), 1.0);
        assert_eq!((-32768i16).to_f32(), -1.0);
    }

    #[test]
    fn u16_to_i16() {
        assert_eq!(32768u16.to_i16(), 0);
        assert_eq!(16384u16.to_i16(), -16384);
        assert_eq!(65535u16.to_i16(), 32767);
        assert_eq!(0u16.to_i16(), -32768);
    }

    #[test]
    fn u16_to_i32() {
        assert_eq!(((std::u16::MAX as f32 / 2.0).round() as u16).to_i32(), 0);
        assert_eq!(std::u16::MAX.to_i32(), std::i32::MAX);
        assert_eq!(std::u16::MIN.to_i32(), std::i32::MIN);
    }

    #[test]
    fn u16_to_u16() {
        assert_eq!(0u16.to_u16(), 0);
        assert_eq!(467u16.to_u16(), 467);
        assert_eq!(32767u16.to_u16(), 32767);
        assert_eq!(65535u16.to_u16(), 65535);
    }

    #[test]
    fn u16_to_f32() {
        assert_eq!(0u16.to_f32(), -1.0);
        assert_eq!(32768u16.to_f32(), 0.0);
        assert_eq!(65535u16.to_f32(), 1.0);
    }

    #[test]
    fn f32_to_i16() {
        assert_eq!(0.0f32.to_i16(), 0);
        assert_eq!((-0.5f32).to_i16(), ::std::i16::MIN / 2);
        assert_eq!(1.0f32.to_i16(), ::std::i16::MAX);
        assert_eq!((-1.0f32).to_i16(), ::std::i16::MIN);
    }

    #[test]
    fn f32_to_i32() {
        assert_eq!(1.0f32.to_i32(), std::i32::MAX);
        assert_eq!(0.5f32.to_i32(), 1073741824);
        assert_eq!(0.25f32.to_i32(), 536870912);
        assert_eq!(0.to_i32(), 0);
        assert_eq!((-0.5f32).to_i32(), std::i32::MIN / 2);
        assert_eq!((-1.0f32).to_i32(), std::i32::MIN);
    }

    #[test]
    fn f32_to_u16() {
        assert_eq!((-1.0f32).to_u16(), 0);
        assert_eq!(0.0f32.to_u16(), 32768);
        assert_eq!(1.0f32.to_u16(), 65535);
    }

    #[test]
    fn f32_to_f32() {
        assert_eq!(0.1f32.to_f32(), 0.1);
        assert_eq!((-0.7f32).to_f32(), -0.7);
        assert_eq!(1.0f32.to_f32(), 1.0);
    }
}

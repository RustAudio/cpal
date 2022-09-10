use std::{mem, fmt::Display, marker::PhantomData, ops::Range};

// Workaround until enums can be used as generic arguments
const NATIVE_ENDIAN: u8 = 0;
const LITTLE_ENDIAN: u8 = 1;
const BIG_ENDIAN: u8 = 2;
type Endianess = u8;

pub use dasp_sample::{I24, I48, U24, U48, Sample, FromSample};

/// Format that each sample has.
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
        matches!(*self, SampleFormat::I8 | SampleFormat::I16 | SampleFormat::I32 | SampleFormat::I64)
    }

    #[inline]
    #[must_use]
    pub fn is_uint(&self) -> bool {
        //matches!(*self, SampleFormat::U8 | SampleFormat::U16 | SampleFormat::U24 | SampleFormat::U32 | SampleFormat::U48 | SampleFormat::U64)
        matches!(*self, SampleFormat::U8 | SampleFormat::U16 | SampleFormat::U32 | SampleFormat::U64)
    }

    #[inline]
    #[must_use]
    pub fn is_float(&self) -> bool {
        matches!(*self, SampleFormat::F32 | SampleFormat::F64)
    }

}

impl Display for SampleFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match * self {
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
        }.fmt(f)
    }
}

pub trait SizedSample: Sample {
    const FORMAT: SampleFormat;
}

impl SizedSample for i8 { const FORMAT: SampleFormat = SampleFormat::I8; }
impl SizedSample for i16 { const FORMAT: SampleFormat = SampleFormat::I16; }
// impl SizedSample for I24 { const FORMAT: SampleFormat = SampleFormat::I24; }
impl SizedSample for i32 { const FORMAT: SampleFormat = SampleFormat::I32; }
// impl SizedSample for I48 { const FORMAT: SampleFormat = SampleFormat::I48; }
impl SizedSample for i64 { const FORMAT: SampleFormat = SampleFormat::I64; }
impl SizedSample for u8 { const FORMAT: SampleFormat = SampleFormat::U8; }
impl SizedSample for u16 { const FORMAT: SampleFormat = SampleFormat::U16; }
// impl SizedSample for U24 { const FORMAT: SampleFormat = SampleFormat::U24; }
impl SizedSample for u32 { const FORMAT: SampleFormat = SampleFormat::U32; }
// impl SizedSample for U48 { const FORMAT: SampleFormat = SampleFormat::U48; }
impl SizedSample for u64 { const FORMAT: SampleFormat = SampleFormat::U64; }
impl SizedSample for f32 { const FORMAT: SampleFormat = SampleFormat::F32; }
impl SizedSample for f64 { const FORMAT: SampleFormat = SampleFormat::F64; }

pub trait ToBytes<const N: usize, const ENDIANESS: u8> {
    fn to_bytes(self) -> [u8; N];
}

pub trait FromBytes<const N: usize, const ENDIANESS: u8> {
    fn from_bytes(bytes: [u8; N]) -> Self;
}

pub trait FromToBytes<const ENDIANESS: Endianess, const STRIDE: usize>: FromBytes<STRIDE, ENDIANESS> + ToBytes<STRIDE, ENDIANESS> {
}


impl ToBytes<1, LITTLE_ENDIAN> for i8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_le_bytes() } }
impl ToBytes<1, BIG_ENDIAN> for i8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_be_bytes() } }
impl ToBytes<1, NATIVE_ENDIAN> for i8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_ne_bytes() } }
impl FromBytes<1, LITTLE_ENDIAN> for i8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<1, BIG_ENDIAN> for i8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<1, NATIVE_ENDIAN> for i8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_ne_bytes(bytes) } }
impl ToBytes<2, LITTLE_ENDIAN> for i16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_le_bytes() } }
impl ToBytes<2, BIG_ENDIAN> for i16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_be_bytes() } }
impl ToBytes<2, NATIVE_ENDIAN> for i16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_ne_bytes() } }
impl FromBytes<2, LITTLE_ENDIAN> for i16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<2, BIG_ENDIAN> for i16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<2, NATIVE_ENDIAN> for i16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_ne_bytes(bytes) } }
impl ToBytes<4, LITTLE_ENDIAN> for i32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_le_bytes() } }
impl ToBytes<4, BIG_ENDIAN> for i32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_be_bytes() } }
impl ToBytes<4, NATIVE_ENDIAN> for i32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_ne_bytes() } }
impl FromBytes<4, LITTLE_ENDIAN> for i32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<4, BIG_ENDIAN> for i32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<4, NATIVE_ENDIAN> for i32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_ne_bytes(bytes) } }
impl ToBytes<8, LITTLE_ENDIAN> for i64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_le_bytes() } }
impl ToBytes<8, BIG_ENDIAN> for i64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_be_bytes() } }
impl ToBytes<8, NATIVE_ENDIAN> for i64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_ne_bytes() } }
impl FromBytes<8, LITTLE_ENDIAN> for i64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<8, BIG_ENDIAN> for i64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<8, NATIVE_ENDIAN> for i64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_ne_bytes(bytes) } }

impl ToBytes<1, LITTLE_ENDIAN> for u8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_le_bytes() } }
impl ToBytes<1, BIG_ENDIAN> for u8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_be_bytes() } }
impl ToBytes<1, NATIVE_ENDIAN> for u8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_ne_bytes() } }
impl FromBytes<1, LITTLE_ENDIAN> for u8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<1, BIG_ENDIAN> for u8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<1, NATIVE_ENDIAN> for u8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_ne_bytes(bytes) } }
impl ToBytes<2, LITTLE_ENDIAN> for u16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_le_bytes() } }
impl ToBytes<2, BIG_ENDIAN> for u16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_be_bytes() } }
impl ToBytes<2, NATIVE_ENDIAN> for u16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_ne_bytes() } }
impl FromBytes<2, LITTLE_ENDIAN> for u16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<2, BIG_ENDIAN> for u16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<2, NATIVE_ENDIAN> for u16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_ne_bytes(bytes) } }
impl ToBytes<4, LITTLE_ENDIAN> for u32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_le_bytes() } }
impl ToBytes<4, BIG_ENDIAN> for u32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_be_bytes() } }
impl ToBytes<4, NATIVE_ENDIAN> for u32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_ne_bytes() } }
impl FromBytes<4, LITTLE_ENDIAN> for u32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<4, BIG_ENDIAN> for u32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<4, NATIVE_ENDIAN> for u32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_ne_bytes(bytes) } }
impl ToBytes<8, LITTLE_ENDIAN> for u64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_le_bytes() } }
impl ToBytes<8, BIG_ENDIAN> for u64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_be_bytes() } }
impl ToBytes<8, NATIVE_ENDIAN> for u64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_ne_bytes() } }
impl FromBytes<8, LITTLE_ENDIAN> for u64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<8, BIG_ENDIAN> for u64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<8, NATIVE_ENDIAN> for u64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_ne_bytes(bytes) } }

impl ToBytes<4, LITTLE_ENDIAN> for f32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_le_bytes() } }
impl ToBytes<4, BIG_ENDIAN> for f32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_be_bytes() } }
impl ToBytes<4, NATIVE_ENDIAN> for f32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_ne_bytes() } }
impl FromBytes<4, LITTLE_ENDIAN> for f32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<4, BIG_ENDIAN> for f32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<4, NATIVE_ENDIAN> for f32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_ne_bytes(bytes) } }
impl ToBytes<8, LITTLE_ENDIAN> for f64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_le_bytes() } }
impl ToBytes<8, BIG_ENDIAN> for f64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_be_bytes() } }
impl ToBytes<8, NATIVE_ENDIAN> for f64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_ne_bytes() } }
impl FromBytes<8, LITTLE_ENDIAN> for f64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<8, BIG_ENDIAN> for f64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<8, NATIVE_ENDIAN> for f64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_ne_bytes(bytes) } }


pub struct RawSampleBuffer<'a, SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> {
    bytes: &'a mut [u8],
    phantom_data: PhantomData<SAMPLE>,
}


trait SampleAccess<SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> {
    fn get_sample(&self, index: usize) -> SAMPLE;
    fn set_sample(&mut self, index: usize, sample: SAMPLE);
    //fn samples<I: Iterator<Item=SAMPLE>>(&self) -> I;
    fn samples(&self) -> Samples<'_, SAMPLE, ENDIANESS, STRIDE>;
    fn samples_mut(&mut self) -> SamplesMut<'_, SAMPLE, ENDIANESS, STRIDE>;
}

impl<'a, SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> RawSampleBuffer<'a, SAMPLE, ENDIANESS, STRIDE> {

    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self {
            bytes,
            phantom_data: PhantomData::default(),
        }
    }

    #[inline]
    const fn get_bytes_range(&self, index: usize) -> Range<usize> {
        let byte_offset = index * STRIDE;
        byte_offset..byte_offset + STRIDE
    }

    #[inline]
    fn get_bytes(&self, index: usize) -> &[u8; STRIDE] {
        let range = self.get_bytes_range(index);
        let sample_bytes = &self.bytes[range];
        <&[u8; STRIDE]>::try_from(sample_bytes).unwrap()
    }

    #[inline]
    fn get_bytes_mut(&mut self, index: usize) -> &mut [u8; STRIDE] {
        let range = self.get_bytes_range(index);
        let sample_bytes = &mut self.bytes[range];
        <&mut [u8; STRIDE]>::try_from(sample_bytes).unwrap()
    }

}

pub struct Samples<'a, SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> {
    bytes: &'a [u8],
    _phantom_data: PhantomData<SAMPLE>,
}

impl<'a, SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> Samples<'a, SAMPLE, ENDIANESS, STRIDE> {

    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            _phantom_data: PhantomData::default(),
        }
    }

}

impl<SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> Iterator for Samples<'_, SAMPLE, ENDIANESS, STRIDE>
where SAMPLE: FromBytes<STRIDE, ENDIANESS>
{
    type Item = SAMPLE;

    fn next(&mut self) -> Option<Self::Item> {
        if self.bytes.len() >= STRIDE {
            let (sample_bytes, remainder) = self.bytes.split_at(STRIDE);
            self.bytes = remainder;
            let sample_bytes = <[u8; STRIDE]>::try_from(sample_bytes).unwrap();
            Some(SAMPLE::from_bytes(sample_bytes))
        } else {
            None
        }
    }
}

struct SamplesMut<'a, SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> {
    bytes: &'a mut [u8],
    _phantom_data: PhantomData<SAMPLE>,
}

impl<'a, SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> SamplesMut<'a, SAMPLE, ENDIANESS, STRIDE>
where
    SAMPLE: FromToBytes<ENDIANESS, STRIDE>
{
    
    fn new(bytes: &'a mut [u8]) -> Self {
        Self {
            bytes,
            _phantom_data: PhantomData::default(),
        }
    }

}

impl<'a, SAMPLE: Sample + FromToBytes<ENDIANESS, STRIDE>, const ENDIANESS: Endianess, const STRIDE: usize> Iterator for SamplesMut<'a, SAMPLE, ENDIANESS, STRIDE> {
    type Item = SampleMut<'a, SAMPLE, ENDIANESS, STRIDE>;

    fn next(&mut self) -> Option<SampleMut<'a, SAMPLE, ENDIANESS, STRIDE>> {
        if self.bytes.len() < STRIDE {
            None
        } else {
            let tmp = std::mem::take(&mut self.bytes);
            let (head, tail) = tmp.split_at_mut(STRIDE);
            self.bytes = tail;
            let sample_bytes = <&mut [u8; STRIDE]>::try_from(head).unwrap();
            Some(SampleMut::new(sample_bytes))
        }
    }

}

pub struct SampleMut<'a, SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> {
    bytes: &'a mut [u8; STRIDE],
    _phantom_data: PhantomData<SAMPLE>,
}

impl<'a, SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> SampleMut<'a, SAMPLE, ENDIANESS, STRIDE>
where
    SAMPLE: FromToBytes<ENDIANESS, STRIDE>
{
    fn new(bytes: &'a mut [u8; STRIDE]) -> Self {
        Self {
            bytes,
            _phantom_data: PhantomData::default(),
        }
    }

    #[inline]
    pub fn get(&self) -> SAMPLE {
        SAMPLE::from_bytes(*self.bytes)
    }

    #[inline]
    pub fn set(&mut self, sample: SAMPLE) {
        *self.bytes = sample.to_bytes();
    }

}

impl<SAMPLE, const ENDIANESS: Endianess, const STRIDE: usize> SampleAccess<SAMPLE, ENDIANESS, STRIDE> for RawSampleBuffer<'_, SAMPLE, ENDIANESS, STRIDE>
where
    SAMPLE: FromToBytes<ENDIANESS, STRIDE>,
{
    #[inline]
    fn get_sample(&self, index: usize) -> SAMPLE {
        SAMPLE::from_bytes(*self.get_bytes(index))
    }

    #[inline]
    fn set_sample(&mut self, index: usize, sample: SAMPLE) {
        *self.get_bytes_mut(index) = sample.to_bytes();
    }

    fn samples(&self) -> Samples<'_, SAMPLE, ENDIANESS, STRIDE> {
        Samples::new(self.bytes)
    }

    fn samples_mut(&mut self) -> SamplesMut<'_, SAMPLE, ENDIANESS, STRIDE> {
        SamplesMut::new(self.bytes)
    }
}

pub enum I8SampleBuffer<'bytes> {
    I8B1(RawSampleBuffer<'bytes, i8, NATIVE_ENDIAN, 1>),
}

// impl I8SampleBuffer<'bytes> {

//     fn access(&self) -> &dyn SampleAccess<I8> {
//     }

// }

pub enum U8SampleBuffer<'bytes> {
    U8B1(RawSampleBuffer<'bytes, u8, NATIVE_ENDIAN, 1>),
}

pub enum I16SampleBuffer<'bytes> {
    I16B2LE(RawSampleBuffer<'bytes, i16, LITTLE_ENDIAN, 2>),
    I16B2BE(RawSampleBuffer<'bytes, i16, BIG_ENDIAN, 2>),
    I16B2NE(RawSampleBuffer<'bytes, i16, NATIVE_ENDIAN, 2>),
}

pub enum U16SampleBuffer<'bytes> {
    U16B2LE(RawSampleBuffer<'bytes, u16, LITTLE_ENDIAN, 2>),
    U16B2BE(RawSampleBuffer<'bytes, u16, BIG_ENDIAN, 2>),
    U16B2NE(RawSampleBuffer<'bytes, u16, NATIVE_ENDIAN, 2>),
}

// currently not supported by `dasp_sample`
// pub enum I18SampleBuffer<'bytes> {
//     I18B3LE(SampleBuffer<'bytes, I18, LITTLE_ENDIAN, 3>),
//     I18B3BE(SampleBuffer<'bytes, I18, BIG_ENDIAN, 3>),
//     I18B3NE(SampleBuffer<'bytes, I18, NATIVE_ENDIAN, 3>),
//     I18B4LE(SampleBuffer<'bytes, I18, LITTLE_ENDIAN, 4>),
//     I18B4BE(SampleBuffer<'bytes, I18, BIG_ENDIAN, 4>),
//     I18B4NE(SampleBuffer<'bytes, I18, NATIVE_ENDIAN, 4>),
// }

// currently not supported by `dasp_sample`
// pub enum U18SampleBuffer<'bytes> {
//     U18B3LE(SampleBuffer<'bytes, U18, LITTLE_ENDIAN, 3>),
//     U18B3BE(SampleBuffer<'bytes, U18, BIG_ENDIAN, 3>),
//     U18B3NE(SampleBuffer<'bytes, U18, NATIVE_ENDIAN, 3>),
//     U18B4LE(SampleBuffer<'bytes, U18, LITTLE_ENDIAN, 4>),
//     U18B4BE(SampleBuffer<'bytes, U18, BIG_ENDIAN, 4>),
//     U18B4NE(SampleBuffer<'bytes, U18, NATIVE_ENDIAN, 4>),
// }

// currently not supported by `dasp_sample`
// pub enum I20SampleBuffer<'bytes> {
//     I20B3LE(SampleBuffer<'bytes, I20, LITTLE_ENDIAN, 3>),
//     I20B3BE(SampleBuffer<'bytes, I20, BIG_ENDIAN, 3>),
//     I20B3NE(SampleBuffer<'bytes, I20, NATIVE_ENDIAN, 3>),
//     I20B4LE(SampleBuffer<'bytes, I20, LITTLE_ENDIAN, 4>),
//     I20B4BE(SampleBuffer<'bytes, I20, BIG_ENDIAN, 4>),
//     I20B4NE(SampleBuffer<'bytes, I20, NATIVE_ENDIAN, 4>),
// }

// currently not supported by `dasp_sample`
// pub enum U20SampleBuffer<'bytes> {
//     U20B3LE(SampleBuffer<'bytes, U20, LITTLE_ENDIAN, 3>),
//     U20B3BE(SampleBuffer<'bytes, U20, BIG_ENDIAN, 3>),
//     U20B3NE(SampleBuffer<'bytes, U20, NATIVE_ENDIAN, 3>),
//     U20B4LE(SampleBuffer<'bytes, U20, LITTLE_ENDIAN, 4>),
//     U20B4BE(SampleBuffer<'bytes, U20, BIG_ENDIAN, 4>),
//     U20B4NE(SampleBuffer<'bytes, U20, NATIVE_ENDIAN, 4>),
// }

pub enum I24SampleBuffer<'bytes> {
    I24B3LE(RawSampleBuffer<'bytes, I24, LITTLE_ENDIAN, 3>),
    I24B3BE(RawSampleBuffer<'bytes, I24, BIG_ENDIAN, 3>),
    I24B3NE(RawSampleBuffer<'bytes, I24, NATIVE_ENDIAN, 3>),
    I24B4LE(RawSampleBuffer<'bytes, I24, LITTLE_ENDIAN, 4>),
    I24B4BE(RawSampleBuffer<'bytes, I24, BIG_ENDIAN, 4>),
    I24B4NE(RawSampleBuffer<'bytes, I24, NATIVE_ENDIAN, 4>),
}

pub enum U24SampleBuffer<'bytes> {
    U24B3LE(RawSampleBuffer<'bytes, U24, LITTLE_ENDIAN, 3>),
    U24B3BE(RawSampleBuffer<'bytes, U24, BIG_ENDIAN, 3>),
    U24B3NE(RawSampleBuffer<'bytes, U24, NATIVE_ENDIAN, 3>),
    U24B4LE(RawSampleBuffer<'bytes, U24, LITTLE_ENDIAN, 4>),
    U24B4BE(RawSampleBuffer<'bytes, U24, BIG_ENDIAN, 4>),
    U24B4NE(RawSampleBuffer<'bytes, U24, NATIVE_ENDIAN, 4>),
}

pub enum I32SampleBuffer<'bytes> {
    I32B4LE(RawSampleBuffer<'bytes, i32, LITTLE_ENDIAN, 4>),
    I32B4BE(RawSampleBuffer<'bytes, i32, BIG_ENDIAN, 4>),
    I32B4NE(RawSampleBuffer<'bytes, i32, NATIVE_ENDIAN, 4>),
}

pub enum U32SampleBuffer<'bytes> {
    U32B4LE(RawSampleBuffer<'bytes, u32, LITTLE_ENDIAN, 4>),
    U32B4BE(RawSampleBuffer<'bytes, u32, BIG_ENDIAN, 4>),
    U32B4NE(RawSampleBuffer<'bytes, u32, NATIVE_ENDIAN, 4>),
}

pub enum F32SampleBuffer<'bytes> {
    F32B4LE(RawSampleBuffer<'bytes, f32, LITTLE_ENDIAN, 4>),
    F32B4BE(RawSampleBuffer<'bytes, f32, BIG_ENDIAN, 4>),
    F32B4NE(RawSampleBuffer<'bytes, f32, NATIVE_ENDIAN, 4>),
}

pub enum F64SampleBuffer<'bytes> {
    F64B4LE(RawSampleBuffer<'bytes, f64, LITTLE_ENDIAN, 8>),
    F64B4BE(RawSampleBuffer<'bytes, f64, BIG_ENDIAN, 8>),
    F64B4NE(RawSampleBuffer<'bytes, f64, NATIVE_ENDIAN, 8>),
}



// enum SampleStorageFormat {
//     I8,
//     I16LE,
//     I16BE,
//     I24LE3,
//     I24BE3,
//     I24LE4,
//     I24BE4,
//     I32LE,
//     I32BE,
//     I48LE6,
//     I48BE6,
//     I48LE8,
//     I48BE8,
//     I64LE,
//     I64BE,
//     U8,
//     U16LE,
//     U16BE,
//     U24LE3,
//     U24BE3,
//     U24LE4,
//     U24BE4,
//     U32LE,
//     U32BE,
//     U48LE6,
//     U48BE6,
//     U48LE8,
//     U48BE8,
//     U64LE,
//     U64BE,
//     F32LE,
//     F32BE,
//     F64LE,
//     F64BE,
// }

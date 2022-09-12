use std::{marker::PhantomData, mem, ops::Range, fmt::Display, array::from_fn};

use dasp_sample::Sample;

pub mod i8;
pub mod i16;
pub mod i32;
pub mod i64;

pub mod u8;
pub mod u16;
pub mod u32;
pub mod u64;

pub mod f32;
pub mod f64;

// Workaround until enums can be used as generic arguments
const NATIVE_ENDIAN: u8 = 0;
const LITTLE_ENDIAN: u8 = 1;
const BIG_ENDIAN: u8 = 2;
type EndiannessU8 = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Endianness {
    Native = NATIVE_ENDIAN,
    Little = LITTLE_ENDIAN,
    Big = BIG_ENDIAN,
}

pub trait ToBytes<const N: usize, const ENDIANNESS: EndiannessU8> {
    fn to_bytes(self) -> [u8; N];
}

pub trait FromBytes<const N: usize, const ENDIANNESS: EndiannessU8> {
    fn from_bytes(bytes: [u8; N]) -> Self;
}

pub trait FromToBytes<const ENDIANNESS: EndiannessU8, const STRIDE: usize>: FromBytes<STRIDE, ENDIANNESS> + ToBytes<STRIDE, ENDIANNESS> {
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SampleFormat {
    I8B1,
    I16B2(Endianness),
    I32B4(Endianness),
    
    U8B1,
    U16B2(Endianness),
    U32B4(Endianness),
    
    F32B4(Endianness),
    F64B8(Endianness),
}

impl SampleFormat {

    pub fn sample_size(self) -> usize {
        match self {
            SampleFormat::I8B1 => 1,
            SampleFormat::I16B2(_) => 2,
            SampleFormat::I32B4(_) => 4,
            SampleFormat::U8B1 => 1,
            SampleFormat::U16B2(_) => 2,
            SampleFormat::U32B4(_) => 4,
            SampleFormat::F32B4(_) => 4,
            SampleFormat::F64B8(_) => 8,
        }
    }

    pub fn is_f32(self) -> bool {
        matches!(self, SampleFormat::F32B4(_))
    }

    pub fn is_i16(self) -> bool {
        matches!(self, SampleFormat::I16B2(_))
    }

    pub fn is_u16(self) -> bool {
        matches!(self, SampleFormat::U16B2(_))
    }

}

impl Endianness {

    pub fn is_big(self) -> bool {
        match self {
            Endianness::Native => cfg!(target_endian = "big"),
            Endianness::Little => false,
            Endianness::Big => true,
        }
    }

    pub fn is_little(self) -> bool {
        match self {
            Endianness::Native => cfg!(target_endian = "little"),
            Endianness::Little => true,
            Endianness::Big => false,
        }
    }

}

impl Display for Endianness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Endianness::Native => "ne",
            Endianness::Little => "le",
            Endianness::Big => "be",
        }.fmt(f)
    }
}


impl Display for SampleFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::I8B1 => "i8b1".to_string(),
            Self::I16B2(endianness) => format!("i16b2{endianness}"),
            Self::I32B4(endianness) => format!("i32b4{endianness}"),
            Self::U8B1 => "u8".to_string(),
            Self::U16B2(endianness) => format!("u16b2{endianness}"),
            Self::U32B4(endianness) => format!("u32b4{endianness}"),
            Self::F32B4(endianness) => format!("f32b4{endianness}"),
            Self::F64B8(endianness) => format!("f64b8{endianness}"),
        }.fmt(f)
    }
}


/// Describes how to read/write a stream of samples from/to a byte-backed buffer
pub trait Transcoder {
    type Sample: Sample;
    const STRIDE: usize;
    const ENDIANNESS: EndiannessU8;
    type Bytes: Copy;
    const FORMAT: SampleFormat;

    fn slice_to_bytes(bytes: &[u8]) -> Self::Bytes;
    fn slice_to_bytes_mut(bytes: &mut [u8]) -> &mut Self::Bytes;
    fn bytes_to_sample(bytes: Self::Bytes) -> Self::Sample;
    fn sample_to_bytes(sample: Self::Sample) -> Self::Bytes;

    #[inline]
    fn slice_to_sample(bytes: &[u8]) -> Self::Sample {
        Self::bytes_to_sample(Self::slice_to_bytes(bytes))
    }

}

/// wraps a byte buffer and provides an iterator over samples
pub struct SampleReader<'buffer, T: Transcoder> {
    buffer: &'buffer [u8],
    phantom_data: PhantomData<T>,
}

impl<'buffer, T: Transcoder> SampleReader<'buffer, T> {

    fn new(buffer: &'buffer[u8]) -> Self {
        Self {
            buffer,
            phantom_data: PhantomData::default(),
        }
    }

}

impl<'buffer, T: Transcoder> Iterator for SampleReader<'buffer, T> {
    type Item = T::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        (self.buffer.len() >= T::STRIDE).then( || {
            let (sample_bytes, remainder) = self.buffer.split_at(T::STRIDE);
            self.buffer = remainder;
            T::bytes_to_sample(T::slice_to_bytes(sample_bytes))
        })
    }

    // TODO implement more iterator methods and impl more iterator traits
}

/// wraps a byte buffer and provides an iterator that gives write access to each sample
pub struct SampleWriter<'buffer, T: Transcoder> {
    buffer: &'buffer mut [u8],
    phantom_data: PhantomData<T>,
}

impl<'buffer, T: Transcoder> SampleWriter<'buffer, T> {

    fn new(buffer: &'buffer mut [u8]) -> Self {
        Self {
            buffer,
            phantom_data: PhantomData::default(),
        }
    }

    pub fn remaining(&self) -> usize {
        self.buffer.len() / T::STRIDE
    }

    pub fn frame_array<const COUNT: usize>(&'buffer mut self) -> Option<[<Self as Iterator>::Item; COUNT]> {
        (self.remaining() >= COUNT).then(|| {
            from_fn(|_| self.next().unwrap())
        })
    }

    pub fn write_iter(&mut self, source: impl Iterator<Item = T::Sample>) -> usize {
        self.buffer.chunks_mut(T::STRIDE).zip(source)
                .map(|(bytes, sample)| *T::slice_to_bytes_mut(bytes) = T::sample_to_bytes(sample))
                .count()
    }

    pub fn frames(&'buffer mut self, count: usize) -> Option<SampleWriter<'buffer, T>> {
        (self.remaining() >= count).then(|| {
            let tmp = mem::take(&mut self.buffer);
            let (frame_bytes, remainder) = tmp.split_at_mut(count * T::STRIDE);
            self.buffer = remainder;
            SampleWriter::new(frame_bytes)
        })
    }

}

impl<'buffer, T: Transcoder> Iterator for SampleWriter<'buffer, T>
where
    T::Bytes: 'buffer,
{
    type Item = SampleMut<'buffer, T>;

    fn next(&mut self) -> Option<Self::Item> {
        (self.buffer.len() >= T::STRIDE).then( || {
            let tmp = mem::take(&mut self.buffer);
            let (sample_bytes, remainder) = tmp.split_at_mut(T::STRIDE);
            self.buffer = remainder;
            SampleMut::new(T::slice_to_bytes_mut(sample_bytes))
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.remaining();
        (size, Some(size))
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        let skip = n.min(self.remaining());
        let tmp = mem::take(&mut self.buffer);
        self.buffer = &mut tmp[(skip * T::STRIDE)..];
        self.next()
    }

    // TODO implement more iterator methods and impl more iterator traits
}

impl<'buffer, T: Transcoder> ExactSizeIterator for SampleWriter<'buffer, T>
where
    T::Bytes: 'buffer,
{
}

/// provides write access to a single byte-backed sample
pub struct SampleMut<'buffer, T: Transcoder> {
    bytes: &'buffer mut T::Bytes,
    _phantom_data: PhantomData<T>,
}

impl<'a, T: Transcoder> SampleMut<'a, T> {
    fn new(bytes: &'a mut T::Bytes) -> Self {
        Self {
            bytes,
            _phantom_data: PhantomData::default(),
        }
    }

    #[inline]
    pub fn get(&self) -> T::Sample {
        T::bytes_to_sample(*self.bytes)
    }

    #[inline]
    pub fn set(&mut self, sample: T::Sample) {
        *self.bytes = T::sample_to_bytes(sample);
    }

}

pub struct SampleBuffer<'buffer, T: Transcoder> {
    bytes: &'buffer [u8],
    phantom_data: PhantomData<T>,
}

impl<'buffer, T: Transcoder> SampleBuffer<'buffer, T> {

    pub fn new(bytes: &'buffer [u8]) -> Self {
        Self {
            bytes,
            phantom_data: PhantomData::default(),
        }
    }

    #[inline]
    fn index(index: usize) -> usize {
        index * T::STRIDE
    }

    #[inline]
    fn index_range(index: usize) -> Range<usize> {
        let start = Self::index(index);
        start..(start + T::STRIDE)
    }

    #[inline]
    fn get_sample_bytes(&self, index: usize) -> Option<&[u8]> {
        self.bytes.get(Self::index_range(index))
    }

}

impl<'buffer, T: Transcoder> BufferReadAccess<T::Sample> for SampleBuffer<'buffer, T> {
    #[inline]
    fn len(&self) -> usize {
        self.bytes.len() / T::STRIDE
    }

    #[inline]
    fn get(&self, index: usize) -> Option<T::Sample> {
        self.get_sample_bytes(index).map(T::slice_to_sample)
    }
}

impl<'buffer, T: Transcoder> IntoIterator for SampleBuffer<'buffer, T> {
    type Item = T::Sample;

    type IntoIter = SampleReader<'buffer, T>;

    fn into_iter(self) -> Self::IntoIter {
        SampleReader::new(self.bytes)
    }
}

impl<'buffer, T: Transcoder> IntoIterator for &SampleBuffer<'buffer, T> {
    type Item = T::Sample;

    type IntoIter = SampleReader<'buffer, T>;

    fn into_iter(self) -> Self::IntoIter {
        SampleReader::new(self.bytes)
    }
}

pub struct SampleBufferMut<'buffer, T: Transcoder> {
    bytes: &'buffer mut [u8],
    phantom_data: PhantomData<T>,
}

impl<'buffer, T: Transcoder> SampleBufferMut<'buffer, T> {

    pub fn new(bytes: &'buffer mut [u8]) -> Self {
        Self {
            bytes,
            phantom_data: PhantomData::default(),
        }
    }

    #[inline]
    fn index(index: usize) -> usize {
        index * T::STRIDE
    }

    #[inline]
    fn index_range(index: usize) -> Range<usize> {
        let start = Self::index(index);
        start..(start + T::STRIDE)
    }

    #[inline]
    fn get_sample_bytes(&self, index: usize) -> Option<&[u8]> {
        self.bytes.get(Self::index_range(index))
    }

    #[inline]
    fn get_sample_bytes_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        self.bytes.get_mut(Self::index_range(index))
    }

    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<SampleMut<'_, T>> {
        self.get_sample_bytes_mut(index)
                .map(|bytes| SampleMut::new(T::slice_to_bytes_mut(bytes)))
    }

}

impl<'buffer, T: Transcoder> BufferReadAccess<T::Sample> for SampleBufferMut<'buffer, T> {
    #[inline]
    fn len(&self) -> usize {
        self.bytes.len() / T::STRIDE
    }

    #[inline]
    fn get(&self, index: usize) -> Option<T::Sample> {
        self.get_sample_bytes(index).map(T::slice_to_sample)
    }
}

impl<'buffer, T: Transcoder> BufferWriteAccess<T::Sample> for SampleBufferMut<'buffer, T> {
    #[inline]
    fn set(&mut self, index: usize, sample: T::Sample) {
        if let Some(bytes) = self.get_sample_bytes_mut(index) {
            *T::slice_to_bytes_mut(bytes) = T::sample_to_bytes(sample);
        }
    }
}

impl<'buffer, T: Transcoder> IntoIterator for SampleBufferMut<'buffer, T>
where
    T: 'buffer,
{
    type Item = SampleMut<'buffer, T>;

    type IntoIter = SampleWriter<'buffer, T>;

    fn into_iter(self) -> Self::IntoIter {
        SampleWriter::new(self.bytes)
    }
}

impl<'buffer, T: Transcoder> IntoIterator for &'buffer mut SampleBufferMut<'buffer, T>
where
    T: 'buffer,
{
    type Item = SampleMut<'buffer, T>;

    type IntoIter = SampleWriter<'buffer, T>;

    fn into_iter(self) -> Self::IntoIter {
        SampleWriter::new(self.bytes)
    }
}


pub trait BufferReadAccess<S> {
    fn len(&self) -> usize;
    fn get(&self, index: usize) -> Option<S>;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub trait BufferWriteAccess<S> {
    fn set(&mut self, index: usize, sample: S);
}


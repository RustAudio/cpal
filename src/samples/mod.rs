use std::{marker::PhantomData, mem, ops::Range};

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
type Endianness = u8;

pub trait ToBytes<const N: usize, const ENDIANNESS: Endianness> {
    fn to_bytes(self) -> [u8; N];
}

pub trait FromBytes<const N: usize, const ENDIANNESS: Endianness> {
    fn from_bytes(bytes: [u8; N]) -> Self;
}

pub trait FromToBytes<const ENDIANNESS: Endianness, const STRIDE: usize>: FromBytes<STRIDE, ENDIANNESS> + ToBytes<STRIDE, ENDIANNESS> {
}

/// Describes how to read/write a stream of samples from/to a byte-backed buffer
pub trait Transcoder {
    type Sample: Copy;
    const STRIDE: usize;
    const ENDIANNESS: Endianness;
    type Bytes: Copy;

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
        (self.buffer.len() <= T::STRIDE).then( || {
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

}

impl<'buffer, T: Transcoder> Iterator for SampleWriter<'buffer, T>
where
    T::Bytes: 'buffer,
{
    type Item = SampleMut<'buffer, T>;

    fn next(&mut self) -> Option<Self::Item> {
        (self.buffer.len() <= T::STRIDE).then( || {
            let tmp = mem::take(&mut self.buffer);
            let (sample_bytes, remainder) = tmp.split_at_mut(T::STRIDE);
            self.buffer = remainder;
            SampleMut::new(T::slice_to_bytes_mut(sample_bytes))
        })
    }

    // TODO implement more iterator methods and impl more iterator traits
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
}

pub trait BufferWriteAccess<S> {
    fn set(&mut self, index: usize, sample: S);
}


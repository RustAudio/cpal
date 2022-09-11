use super::{ToBytes, FromBytes, LITTLE_ENDIAN, BIG_ENDIAN, NATIVE_ENDIAN, Transcoder, Endianness, SampleBuffer, SampleBufferMut, BufferReadAccess, BufferWriteAccess};

type Sample = i16;

impl ToBytes<2, LITTLE_ENDIAN> for Sample { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_le_bytes() } }
impl ToBytes<2, BIG_ENDIAN> for Sample { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_be_bytes() } }
impl ToBytes<2, NATIVE_ENDIAN> for Sample { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_ne_bytes() } }
impl FromBytes<2, LITTLE_ENDIAN> for Sample { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<2, BIG_ENDIAN> for Sample { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<2, NATIVE_ENDIAN> for Sample { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_ne_bytes(bytes) } }

pub enum I16SampleBuffer<'buffer> {
    B2LE(SampleBuffer<'buffer, B2LE>),
    B2BE(SampleBuffer<'buffer, B2BE>),
    B2NE(SampleBuffer<'buffer, B2NE>),
}

impl<'buffer> BufferReadAccess<Sample> for I16SampleBuffer<'buffer> {

    #[inline]
    fn len(&self) -> usize {
        match *self {
            Self::B2LE(ref buffer) => buffer.len(),
            Self::B2BE(ref buffer) => buffer.len(),
            Self::B2NE(ref buffer) => buffer.len(),
        }
    }

    #[inline]
    fn get(&self, index: usize) -> Option<Sample> {
        match *self {
            Self::B2LE(ref buffer) => buffer.get(index),
            Self::B2BE(ref buffer) => buffer.get(index),
            Self::B2NE(ref buffer) => buffer.get(index),
        }
    }

}

impl<'buffer> IntoIterator for I16SampleBuffer<'buffer> {
    type Item = Sample;

    type IntoIter = Box<dyn Iterator<Item = Sample> + 'buffer>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            I16SampleBuffer::B2LE(ref buffer) => Box::new(buffer.into_iter()),
            I16SampleBuffer::B2BE(ref buffer) => Box::new(buffer.into_iter()),
            I16SampleBuffer::B2NE(ref buffer) => Box::new(buffer.into_iter()),
        }
    }
}

impl<'buffer> IntoIterator for &I16SampleBuffer<'buffer> {
    type Item = Sample;

    type IntoIter = Box<dyn Iterator<Item = Sample> + 'buffer>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            I16SampleBuffer::B2LE(ref buffer) => Box::new(buffer.into_iter()),
            I16SampleBuffer::B2BE(ref buffer) => Box::new(buffer.into_iter()),
            I16SampleBuffer::B2NE(ref buffer) => Box::new(buffer.into_iter()),
        }
    }
}

pub enum I16SampleBufferMut<'buffer> {
    B2LE(SampleBufferMut<'buffer, B2LE>),
    B2BE(SampleBufferMut<'buffer, B2BE>),
    B2NE(SampleBufferMut<'buffer, B2NE>),
}

impl<'buffer> BufferReadAccess<Sample> for I16SampleBufferMut<'buffer> {

    #[inline]
    fn len(&self) -> usize {
        match *self {
            Self::B2LE(ref buffer) => buffer.len(),
            Self::B2BE(ref buffer) => buffer.len(),
            Self::B2NE(ref buffer) => buffer.len(),
        }
    }

    #[inline]
    fn get(&self, index: usize) -> Option<Sample> {
        match *self {
            Self::B2LE(ref buffer) => buffer.get(index),
            Self::B2BE(ref buffer) => buffer.get(index),
            Self::B2NE(ref buffer) => buffer.get(index),
        }
    }

}

impl<'buffer> BufferWriteAccess<Sample> for I16SampleBufferMut<'buffer> {

    #[inline]
    fn set(&mut self, index: usize, sample: Sample) {
        match *self {
            Self::B2LE(ref mut buffer) => buffer.set(index, sample),
            Self::B2BE(ref mut buffer) => buffer.set(index, sample),
            Self::B2NE(ref mut buffer) => buffer.set(index, sample),
        }
    }

}

pub struct B2LE {}

impl Transcoder for B2LE {
    type Sample = Sample;
    const STRIDE: usize = 2;
    const ENDIANNESS: Endianness = LITTLE_ENDIAN;
    type Bytes = [u8; 2];

    fn slice_to_bytes(bytes: &[u8]) -> Self::Bytes {
        Self::Bytes::try_from(bytes).unwrap()
    }

    fn slice_to_bytes_mut(bytes: &mut[u8]) -> &mut Self::Bytes {
        <&mut Self::Bytes>::try_from(bytes).unwrap()
    }

    fn bytes_to_sample(bytes: Self::Bytes) -> Self::Sample {
        <Self::Sample as FromBytes::<{Self::STRIDE}, {Self::ENDIANNESS}>>::from_bytes(bytes)
    }

    fn sample_to_bytes(sample: Self::Sample) -> Self::Bytes {
        <Self::Sample as ToBytes::<{Self::STRIDE}, {Self::ENDIANNESS}>>::to_bytes(sample)
    }

}

pub struct B2BE {}

impl Transcoder for B2BE {
    type Sample = Sample;
    const STRIDE: usize = 2;
    const ENDIANNESS: Endianness = BIG_ENDIAN;
    type Bytes = [u8; 2];

    fn slice_to_bytes(bytes: &[u8]) -> Self::Bytes {
        Self::Bytes::try_from(bytes).unwrap()
    }

    fn slice_to_bytes_mut(bytes: &mut[u8]) -> &mut Self::Bytes {
        <&mut Self::Bytes>::try_from(bytes).unwrap()
    }

    fn bytes_to_sample(bytes: Self::Bytes) -> Self::Sample {
        <Self::Sample as FromBytes::<{Self::STRIDE}, {Self::ENDIANNESS}>>::from_bytes(bytes)
    }

    fn sample_to_bytes(sample: Self::Sample) -> Self::Bytes {
        <Self::Sample as ToBytes::<{Self::STRIDE}, {Self::ENDIANNESS}>>::to_bytes(sample)
    }

}

pub struct B2NE {}

impl Transcoder for B2NE {
    type Sample = Sample;
    const STRIDE: usize = 2;
    const ENDIANNESS: Endianness = NATIVE_ENDIAN;
    type Bytes = [u8; 2];

    fn slice_to_bytes(bytes: &[u8]) -> Self::Bytes {
        Self::Bytes::try_from(bytes).unwrap()
    }

    fn slice_to_bytes_mut(bytes: &mut[u8]) -> &mut Self::Bytes {
        <&mut Self::Bytes>::try_from(bytes).unwrap()
    }

    fn bytes_to_sample(bytes: Self::Bytes) -> Self::Sample {
        <Self::Sample as FromBytes::<{Self::STRIDE}, {Self::ENDIANNESS}>>::from_bytes(bytes)
    }

    fn sample_to_bytes(sample: Self::Sample) -> Self::Bytes {
        <Self::Sample as ToBytes::<{Self::STRIDE}, {Self::ENDIANNESS}>>::to_bytes(sample)
    }

}

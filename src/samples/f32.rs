use super::{ToBytes, FromBytes, LITTLE_ENDIAN, BIG_ENDIAN, Transcoder, EndiannessU8, SampleBuffer, SampleBufferMut, BufferReadAccess, BufferWriteAccess, SampleFormat, Endianness};

type Sample = f32;

impl ToBytes<4, LITTLE_ENDIAN> for Sample { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_le_bytes() } }
impl ToBytes<4, BIG_ENDIAN> for Sample { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_be_bytes() } }
impl FromBytes<4, LITTLE_ENDIAN> for Sample { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<4, BIG_ENDIAN> for Sample { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_be_bytes(bytes) } }

pub enum F32SampleBuffer<'buffer> {
    B4LE(SampleBuffer<'buffer, B4LE>),
    B4BE(SampleBuffer<'buffer, B4BE>),
    B4NE(SampleBuffer<'buffer, B4NE>),
}

impl<'buffer> BufferReadAccess<Sample> for F32SampleBuffer<'buffer> {

    #[inline]
    fn len(&self) -> usize {
        match *self {
            Self::B4LE(ref buffer) => buffer.len(),
            Self::B4BE(ref buffer) => buffer.len(),
            Self::B4NE(ref buffer) => buffer.len(),
        }
    }

    #[inline]
    fn get(&self, index: usize) -> Option<Sample> {
        match *self {
            Self::B4LE(ref buffer) => buffer.get(index),
            Self::B4BE(ref buffer) => buffer.get(index),
            Self::B4NE(ref buffer) => buffer.get(index),
        }
    }

}

impl<'buffer> IntoIterator for F32SampleBuffer<'buffer> {
    type Item = Sample;

    type IntoIter = Box<dyn Iterator<Item = Sample> + 'buffer>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            F32SampleBuffer::B4LE(ref buffer) => Box::new(buffer.into_iter()),
            F32SampleBuffer::B4BE(ref buffer) => Box::new(buffer.into_iter()),
            F32SampleBuffer::B4NE(ref buffer) => Box::new(buffer.into_iter()),
        }
    }
}

impl<'buffer> IntoIterator for &F32SampleBuffer<'buffer> {
    type Item = Sample;

    type IntoIter = Box<dyn Iterator<Item = Sample> + 'buffer>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            F32SampleBuffer::B4LE(ref buffer) => Box::new(buffer.into_iter()),
            F32SampleBuffer::B4BE(ref buffer) => Box::new(buffer.into_iter()),
            F32SampleBuffer::B4NE(ref buffer) => Box::new(buffer.into_iter()),
        }
    }
}

pub enum F32SampleBufferMut<'buffer> {
    B4LE(SampleBufferMut<'buffer, B4LE>),
    B4BE(SampleBufferMut<'buffer, B4BE>),
    B4NE(SampleBufferMut<'buffer, B4NE>),
}

impl<'buffer> BufferReadAccess<Sample> for F32SampleBufferMut<'buffer> {

    #[inline]
    fn len(&self) -> usize {
        match *self {
            Self::B4LE(ref buffer) => buffer.len(),
            Self::B4BE(ref buffer) => buffer.len(),
            Self::B4NE(ref buffer) => buffer.len(),
        }
    }

    #[inline]
    fn get(&self, index: usize) -> Option<Sample> {
        match *self {
            Self::B4LE(ref buffer) => buffer.get(index),
            Self::B4BE(ref buffer) => buffer.get(index),
            Self::B4NE(ref buffer) => buffer.get(index),
        }
    }

}

impl<'buffer> BufferWriteAccess<Sample> for F32SampleBufferMut<'buffer> {

    #[inline]
    fn set(&mut self, index: usize, sample: Sample) {
        match *self {
            Self::B4LE(ref mut buffer) => buffer.set(index, sample),
            Self::B4BE(ref mut buffer) => buffer.set(index, sample),
            Self::B4NE(ref mut buffer) => buffer.set(index, sample),
        }
    }

}

pub struct B4LE {}

impl Transcoder for B4LE {
    type Sample = Sample;
    const STRIDE: usize = 4;
    const ENDIANNESS: EndiannessU8 = LITTLE_ENDIAN;
    type Bytes = [u8; 4];
    const FORMAT: SampleFormat = SampleFormat::F32B4(Endianness::Little);

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

pub struct B4BE {}

impl Transcoder for B4BE {
    type Sample = Sample;
    const STRIDE: usize = 4;
    const ENDIANNESS: EndiannessU8 = BIG_ENDIAN;
    type Bytes = [u8; 4];
    const FORMAT: SampleFormat = SampleFormat::F32B4(Endianness::Big);

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

#[cfg(target_endian = "big")]
pub type B4NE = B4BE;
#[cfg(target_endian = "little")]
pub type B4NE = B4LE;

use std::{
    ops::{Range, RangeInclusive},
    sync::Arc,
};

use js_sys::{Atomics, DataView, Float32Array, Int32Array, SharedArrayBuffer};

use crate::BackendSpecificError;

use super::map_js_err;

// Float32Array.BYTES_PER_ELEMENT = 4
// Int32Array.BYTES_PER_ELEMENT = 4
const BYTE_SIZE: u32 = 4;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum BufferState {
    /// Empty chunk
    None = 0,
    /// Written chunk
    Write = 1,
    /// Read chunk
    Read = 2,
}

impl TryFrom<i32> for BufferState {
    type Error = BufferError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BufferState::None),
            1 => Ok(BufferState::Write),
            2 => Ok(BufferState::Read),
            _ => Err(BufferError::InvalidData),
        }
    }
}

#[derive(Debug)]
pub enum BufferError {
    InvalidData,
    OutOfRange,
    BufferFull,
    BufferEmpty,
    BackendSpecific { err: BackendSpecificError },
}

impl From<BackendSpecificError> for BufferError {
    fn from(err: BackendSpecificError) -> Self {
        Self::BackendSpecific { err }
    }
}

/// Chunked buffer
///
/// ```pseudo
/// ((f32;N);S), (i32; N)
/// ^ samples ^ ^read order^
/// ^  chunk  ^
/// ```
///
pub struct AtomicBuffer {
    /// memory shared between main thread and worklet
    shared: Arc<SharedArrayBuffer>,
    /// integer array over the shared memory to work with Atomics
    ints: Arc<Int32Array>,
    /// size of a chunk
    chunk_size: u32,
    /// total chunks
    chunks: u32,
    /// index of the read order
    read_order_index: u32,
    /// hold one place for floats conversion
    _float: Float32Array,
    /// view over float to convert them into ints and back
    view: DataView,
}

impl AtomicBuffer {
    pub fn new(chunks: u32, chunk_size: u32) -> Self {
        let size = chunks * chunk_size;
        assert!(size > 0);

        let shared = SharedArrayBuffer::new((size + chunks) * BYTE_SIZE);
        let ints = Int32Array::new(&shared);
        let read_order_index = ints.length() - chunks;

        for i in 0..chunks {
            Atomics::store(&ints, read_order_index + i, -1).unwrap();
        }

        let float = Float32Array::new_with_length(1);
        let view = DataView::new(&float.buffer(), 0, float.byte_length() as usize);

        Self {
            chunk_size,
            chunks,
            read_order_index,
            _float: float,
            view,
            shared: Arc::new(shared),
            ints: Arc::new(ints),
        }
    }

    fn read_chunks_iter(&self) -> impl Iterator<Item = (u32, RangeInclusive<u32>)> + '_ {
        (0..self.chunks)
            .filter_map(|i| {
                let val = Atomics::load(&self.ints, self.read_order_index + i).unwrap();
                if val >= 0 {
                    Some(val as u32)
                } else {
                    None
                }
            })
            .map(|i| {
                let start = i * self.chunk_size;
                let end = start + self.chunk_size - 1;

                (i, start..=end)
            })
    }

    fn write_chunks_iter(&self) -> impl Iterator<Item = (u32, RangeInclusive<u32>)> + '_ {
        (0..self.chunks)
            .filter_map(|i| {
                if self
                    .read_chunks_iter()
                    .find(|(chunk, _)| chunk == &i)
                    .is_some()
                {
                    None
                } else {
                    Some(i)
                }
            })
            .map(|i| {
                let start = i * self.chunk_size;
                let end = start + self.chunk_size - 1;

                (i, start..=end)
            })
    }

    fn update_read_order_after_reading(&self, _read_idx: u32) -> Result<(), BufferError> {
        for (i, idx) in (0..self.chunks)
            .map(|i| Atomics::load(&self.ints, self.read_order_index + i).unwrap())
            .enumerate()
            .skip(1)
        {
            Atomics::store(&self.ints, self.read_order_index + i as u32 - 1, idx)
                .map_err(map_js_err::<BufferError>)?;
        }

        Atomics::store(&self.ints, self.read_order_index + self.chunks - 1, -1)
            .map_err(map_js_err::<BufferError>)?;

        Atomics::notify(&self.ints, self.read_order_index).map_err(map_js_err::<BufferError>)?;

        Ok(())
    }

    fn update_read_order_after_writing(&self, write_idx: u32) -> Result<(), BufferError> {
        for (i, idx) in (0..self.chunks)
            .map(|i| Atomics::load(&self.ints, self.read_order_index + i).unwrap())
            .enumerate()
        {
            if idx == -1 {
                Atomics::store(
                    &self.ints,
                    self.read_order_index + i as u32,
                    write_idx as i32,
                )
                .map_err(map_js_err::<BufferError>)?;

                return Ok(());
            } else if idx as u32 == write_idx {
                return Err(BufferError::InvalidData);
            }
        }

        Err(BufferError::BufferFull)
    }

    pub fn read(&self, output: &mut [f32]) -> Result<(), BufferError> {
        if let Some((idx, read_rng)) = self.read_chunks_iter().next() {
            for (pos, out_pos) in read_rng.zip(0..self.chunk_size) {
                let int = Atomics::load(&self.ints, pos).map_err(map_js_err::<BufferError>)?;

                self.view.set_int32_endian(0, int, true);
                let float = self.view.get_float32_endian(0, true);

                if let Some(read_into) = output.get_mut(out_pos as usize) {
                    *read_into = float;
                } else {
                    return Err(BufferError::OutOfRange);
                }
            }

            self.update_read_order_after_reading(idx)?;

            Ok(())
        } else {
            Err(BufferError::BufferEmpty)
        }
    }

    pub fn write(&self, input: &[f32]) -> Result<(), BufferError> {
        if let Some((idx, write_rng)) = self.write_chunks_iter().next() {
            for (pos, in_pos) in write_rng.zip(0..self.chunk_size) {
                if let Some(write_from) = input.get(in_pos as usize) {
                    self.view.set_float32_endian(0, *write_from, true);
                    let int = self.view.get_int32_endian(0, true);
                    Atomics::store(&self.ints, pos, int).map_err(map_js_err::<BufferError>)?;
                } else {
                    return Err(BufferError::OutOfRange);
                }
            }

            self.update_read_order_after_writing(idx)?;

            Ok(())
        } else {
            Err(BufferError::BufferFull)
        }
    }

    pub fn chunks_to_read_count(&self) -> u32 {
        self.read_chunks_iter().count() as u32
    }

    pub fn chunks_to_write_count(&self) -> u32 {
        self.write_chunks_iter().count() as u32
    }

    pub fn shared(&self) -> Arc<SharedArrayBuffer> {
        self.shared.clone()
    }
}

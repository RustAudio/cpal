use std::sync::Arc;

use js_sys::{Atomics, DataView, Float32Array, Int32Array, SharedArrayBuffer};

use crate::BackendSpecificError;

use super::map_js_err;

// Float32Array.BYTES_PER_ELEMENT = 4
// Int32Array.BYTES_PER_ELEMENT = 4
const BYTE_SIZE: u32 = 4;

pub enum Direction {
    /// worklet will store the data from its input
    /// main will read the data
    InputFromWorklet,
    /// main will store the data from its callback
    /// worklet will read the data
    OutputFromMain,
}

#[derive(Debug)]
pub enum BufferError {
    InvalidData,
    OutOfRange,
    BackendSpecific { err: BackendSpecificError },
}

impl From<BackendSpecificError> for BufferError {
    fn from(err: BackendSpecificError) -> Self {
        Self::BackendSpecific { err }
    }
}

/// Unidirectional chunked buffer
/// (-f32-)...(-f32-)(write_to)(read_from)
pub struct AtomicBuffer {
    /// memory shared between main thread and worklet
    shared: Arc<SharedArrayBuffer>,
    /// integer array over the shared memory to work with Atomics
    ints: Arc<Int32Array>,
    /// size of a chunk
    chunk_size: u32,
    /// total chunks
    chunks: u32,
    /// index of int indicating which position to read next
    read_target: u32,
    /// index of int indicating which position to write next
    write_target: u32,
    /// hold one place for floats conversion
    _float: Float32Array,
    /// view over float to convert them into ints and back
    view: DataView,
}

impl AtomicBuffer {
    pub fn new(chunks: u32, chunk_size: u32,) -> Self {
        let size = chunks * chunk_size;
        let shared = SharedArrayBuffer::new((size + 2) * BYTE_SIZE);
        let ints = Int32Array::new(&shared);
        let float = Float32Array::new_with_length(1);
        let view = DataView::new(&float.buffer(), 0, float.byte_length() as usize);

        Self {
            chunk_size,
            chunks,
            _float: float,
            view,
            read_target: ints.length() - 1,
            write_target: ints.length() - 2,
            shared: Arc::new(shared),
            ints: Arc::new(ints),
        }
    }

    pub fn read(&self, output: &mut [f32]) -> Result<(), BufferError> {
        let read_pos = self.chunk_to_read()? * self.chunk_size;
        let read_end = read_pos + self.chunk_size;
        for (pos, out_pos) in (read_pos..=read_end).zip(0..self.chunk_size) {
            let int = Atomics::load(&self.ints, pos).map_err(map_js_err::<BufferError>)?;
            self.view.set_int32_endian(0, int, true);
            let float = self.view.get_float32_endian(0, true);
            if let Some(read_into) = output.get_mut(out_pos as usize) {
                *read_into = float;
            } else {
                return Err(BufferError::OutOfRange);
            }
        }

        self.mark_read_next()?;

        Ok(())
    }

    pub fn write(&self, input: &[f32]) -> Result<(), BufferError> {
        let write_pos = self.chunk_to_write()? * self.chunk_size;
        let write_end = write_pos + self.chunk_size;
        for (pos, in_pos) in (write_pos..=write_end).zip(0..self.chunk_size) {
            if let Some(write_from) = input.get(in_pos as usize) {
                self.view.set_float32_endian(0, *write_from, true);
                let int = self.view.get_int32_endian(0, true);
                Atomics::store(&self.ints, pos, int).map_err(map_js_err::<BufferError>)?;
            } else {
                return Err(BufferError::OutOfRange);
            }
        }

        self.mark_write_next()
    }

    pub fn chunks_to_read_count(&self) -> Result<u32, BufferError> {
      let read = self.chunk_to_read()?;
      Ok(self.chunks - read)
    }
    
    pub fn chunks_to_write_count(&self) -> Result<u32, BufferError> {
      let write = self.chunk_to_write()?;
      let read = self.chunk_to_read()?;
      Ok(read.saturating_sub(write))
    }

    pub fn shared(&self) -> Arc<SharedArrayBuffer> {
        self.shared.clone()
    }

    fn chunk_to_read(&self) -> Result<u32, BufferError> {
        let i = Atomics::load(&self.ints, self.read_target).map_err(map_js_err::<BufferError>)?;

        u32::try_from(i).map_err(|_| BufferError::InvalidData)
    }

    fn mark_read_next(&self) -> Result<(), BufferError> {
        let current = self.chunk_to_read()?;
        let next = if current + 1 >= self.chunks {
            0
        } else {
            current + 1
        };

        let i = i32::try_from(next).map_err(|_| BufferError::InvalidData)?;
            Atomics::store(&self.ints, self.read_target, i).map_err(map_js_err::<BufferError>)?;
            Ok(())
    }

    fn mark_write_next(&self) -> Result<(), BufferError> {
        let current = self.chunk_to_write()?;
        let next = if current + 1 >= self.chunks {
            0
        } else {
            current + 1
        };

        let i = i32::try_from(next).map_err(|_| BufferError::InvalidData)?;
        Atomics::store(&self.ints, self.write_target, i).map_err(map_js_err::<BufferError>)?;
        Ok(())
    }

    fn chunk_to_write(&self) -> Result<u32, BufferError> {
        let i = Atomics::load(&self.ints, self.write_target).map_err(map_js_err::<BufferError>)?;

        u32::try_from(i).map_err(|_| BufferError::InvalidData)
    }
}

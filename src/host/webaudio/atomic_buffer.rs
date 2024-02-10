use std::{
    future::Future,
    ops::{Range, RangeInclusive},
    sync::Arc,
};

use js_sys::{
    Array, ArrayBuffer, Atomics, DataView, Float32Array, Function, Int32Array, Object, Promise,
    Reflect, SharedArrayBuffer,
};
use wasm_bindgen::{closure::Closure, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::AbortSignal;

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
    NotSupported,
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
    /// size of a chunk
    pub chunk_size: u32,
    /// total chunks
    pub chunks: u32,
    /// memory shared between main thread and worklet
    shared: Arc<SharedArrayBuffer>,
    /// integer array over the shared memory to work with Atomics
    ints: Arc<Int32Array>,
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

        // todo: check crossOriginIsolated and fallback to messaging

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

    // fn from_shared_with_size(shared: SharedArrayBuffer, chunk_size: u32) -> Self {
    //     let ints = Int32Array::new(&shared.into());

    //     let float = Float32Array::new_with_length(1);
    //     let view = DataView::new(&float.buffer(), 0, float.byte_length() as usize);

    //     let chunks = ints.length() / chunk_size;
    //     let read_order_index = ints.length() - chunks;

    //     Self {
    //         chunk_size,
    //         chunks,
    //         read_order_index,
    //         _float: float,
    //         view,
    //         shared: Arc::new(shared),
    //         ints: Arc::new(ints),
    //     }
    // }

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

    pub fn await_read(&self) -> Result<Promise, BufferError> {
        let value =
            Atomics::load(&self.ints, self.read_order_index).map_err(map_js_err::<BufferError>)?;
        let obj = Atomics::wait_async(&self.ints, self.read_order_index, value)
            .map_err(map_js_err::<BufferError>)?;

        if Reflect::get(&obj, &"async".into())
            .map(|v| v.as_bool().unwrap_or_default())
            .ok()
            .unwrap_or_default()
        {
            let ints = self.ints.clone();
            let read_order_index = self.read_order_index;
            let value = Reflect::get(&obj, &"value".into()).map_err(map_js_err::<BufferError>)?;
            let promise = Promise::from(value);
            Ok(promise)
        } else if Some("not-equal".to_string())
            == Reflect::get(&obj, &"value".into())
                .map(|v| v.as_string())
                .ok()
                .flatten()
        {
            let promise = Promise::resolve(&JsValue::NULL);
            Ok(promise)
        } else {
            Err(BufferError::NotSupported)
        }
    }

    // pub fn js_value() -> Result<Arc<Object>, BufferError> {
    //     let obj = Arc::new(Object::new());

    //     let load_self = obj.clone();
    //     let load =
    //         Closure::wrap(Box::new(move |shared: JsValue| {
    //             match SharedArrayBuffer::try_from(shared.clone()) {
    //                 Ok(shared) => {
    //                     Reflect::set(&load_self, &"buffer".into(), &shared)
    //                         .map_err(map_js_err::<BufferError>)
    //                         .unwrap();
    //                 }
    //                 Err(_) => {
    //                     let fallback = ArrayBuffer::from(shared);
    //                     Reflect::set(&load_self, &"fallback_buffer".into(), &fallback)
    //                         .map_err(map_js_err::<BufferError>)
    //                         .unwrap();
    //                 }
    //             }
    //         }) as Box<dyn FnMut(JsValue)>);

    //     let read_self = obj.clone();
    //     let read = Closure::wrap(Box::new(move |src: JsValue| {
    //         let src = Array::from(&src);
    //         let channels = src.length();
    //         let frames = Float32Array::from(src.get(0)).length();
    //         let buffer = match Reflect::get(&read_self, &"buffer".into()) {
    //             Ok(shared) => {
    //                 let shared = SharedArrayBuffer::from(shared);
    //                 AtomicBuffer::from_shared_with_size(shared, channels * frames)
    //             }
    //             Err(_) => todo!(),
    //         };

    //         let worklet_buffer = Reflect::get(&read_self, &"worklet_buffer".into()).unwrap();
    //         let worklet_buffer = Array::from(worklet_buffer);
    //         buffer.read(worklet_buffer.it).unwrap();
    //         for ch in 0..channels {
    //             let channel = Float32Array::from(src.get(ch));
    //             for fr in 0..frames {
    //                 let i = fr * channels + ch;
    //                 channel.set(, offset)
    //             }
    //         }

    //         // const frames = output[0].length;
    //         // const channels = output.length;
    //         // // read last output from buffer
    //         // for (let fr = 0; fr < frames; fr++) {
    //         //   for (let ch = 0; ch < channels; ch++) {
    //         //     // frame index
    //         //     const i = fr * channels + ch;
    //         //     // load stored frame
    //         //     const f_int = Atomics.load(this.ints, i);
    //         //     // set on view
    //         //     this.view.setInt32(i * Int32Array.BYTES_PER_ELEMENT, f_int);
    //         //     // get as float
    //         //     const f = this.view.getFloat32(
    //         //       i * Float32Array.BYTES_PER_ELEMENT
    //         //     );
    //         //     // write sample
    //         //     output[ch][fr] = f;
    //         //   }
    //         // }
    //     }) as Box<dyn FnMut(JsValue)>);

    //     Reflect::set(&obj, &"load".into(), load.as_ref()).map_err(map_js_err::<BufferError>)?;
    //     Reflect::set(&obj, &"read".into(), read.as_ref()).map_err(map_js_err::<BufferError>)?;

    //     Ok(obj)
    // }
}

impl Into<JsValue> for &AtomicBuffer {
    fn into(self) -> JsValue {
        todo!()
    }
}

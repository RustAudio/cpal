extern crate asio_sys as sys;

use std;
use Format;
use CreationError;
use StreamData;
use std::marker::PhantomData;
use super::Device;
use::std::cell::Cell;
use UnknownTypeOutputBuffer;
use std::sync::{Mutex, Arc};


pub struct EventLoop{
    asio_stream: Arc<Mutex<Option<sys::AsioStream>>>,
    stream_count: Cell<usize>,
    callbacks: Arc<Mutex<Vec<Box<FnMut(StreamId, StreamData) + Send>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);


pub struct InputBuffer<'a, T: 'a>{
    marker: PhantomData<&'a T>,
}
pub struct OutputBuffer<'a, T: 'a>{
    buffer: &'a mut [T],
}

impl EventLoop {
    pub fn new() -> EventLoop {
        EventLoop{ asio_stream: Arc::new(Mutex::new(None)),
        stream_count: Cell::new(0),
        callbacks: Arc::new(Mutex::new(Vec::new()))}
    }

    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        unimplemented!()
    }

    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        match sys::prepare_stream(&device.driver_name) {
            Ok(stream) => {
                {
                    *self.asio_stream
                        .lock()
                        .unwrap() = Some(stream);
                }
                let count = self.stream_count.get();
                self.stream_count.set(count + 1);
                let asio_stream = self.asio_stream.clone();
                let callbacks = self.callbacks.clone();

                sys::set_callback(move |index| {
                    if let Some(asio_stream) = *asio_stream
                        .lock().unwrap(){
                        let data_slice = std::slice::from_raw_parts_mut(
                            asio_stream.buffer_info.buffers[index as usize] as *mut f32,
                            asio_stream.buffer_size as usize);
                        let buff = OutputBuffer{
                            buffer: data_slice
                        };
                        let callbacks = *callbacks.lock().unwrap();
                        match callbacks.first(){
                            Some(callback) => {
                                callback(
                                    StreamId(count),
                                    StreamData::Output{ 
                                        buffer: UnknownTypeOutputBuffer::F32(
                                                    ::OutputBuffer{ 
                                                        target: Some(super::super::OutputBuffer::Asio(buff))
                                                    })
                                    }
                                    ) 
                            },
                            None => return (),
                        }
                    }
                });
                Ok(StreamId(count))
            },
            Err(ref e) => {
                println!("Error preparing stream: {}", e);
                Err(CreationError::DeviceNotAvailable)
            },
        }
        
    }
    
    pub fn play_stream(&self, stream: StreamId) {
        sys::play();
    }
    
    pub fn pause_stream(&self, stream: StreamId) {
        sys::stop();
    }
    pub fn destroy_stream(&self, stream_id: StreamId) {
        let asio_stream_lock = self.asio_stream.lock().unwrap();
        if let Some(old_stream) = *asio_stream_lock{
            sys::destroy_stream(old_stream);
            *asio_stream_lock = None;
        }
    }
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData) + Send
        {
            self.callbacks
                .lock()
                .unwrap()
                .push(Box::new(callback));
            loop{
                // Might need a sleep here to prevent the loop being
                // removed in --release
            }
        }
}

impl<'a, T> InputBuffer<'a, T> {
    pub fn buffer(&self) -> &[T] {
        unimplemented!()
    }
    pub fn finish(self) {
        unimplemented!()
    }
}

impl<'a, T> OutputBuffer<'a, T> {
    pub fn buffer(&mut self) -> &mut [T] {
        unimplemented!()
    }

    pub fn len(&self) -> usize {
        unimplemented!()
    }

    pub fn finish(self) {
        unimplemented!()
    }
}

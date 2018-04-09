extern crate asio_sys as sys;

use Format;
use CreationError;
use StreamData;
use std::marker::PhantomData;
use super::Device;
use::std::cell::Cell;
use::std::cell::RefCell;

pub struct EventLoop{
    asio_buffer_info: RefCell<Option<sys::AsioBufferInfo>>,
    stream_count: Cell<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);


pub struct InputBuffer<'a, T: 'a>{
    marker: PhantomData<&'a T>,
}
pub struct OutputBuffer<'a, T: 'a>{
    marker: PhantomData<&'a T>,
}

impl EventLoop {
    pub fn new() -> EventLoop {
        EventLoop{ asio_buffer_info: RefCell::new(None),
        stream_count: Cell::new(0)}
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
                *self.asio_buffer_info.borrow_mut() = Some(stream.buffer_info);
                let count = self.stream_count.get();
                self.stream_count.set(count + 1);
                Ok(StreamId(count))
            },
            Err(ref e) => {
                println!("Errror preparing stream: {}", e);
                Err(CreationError::DeviceNotAvailable)
            },
        }
        
    }
    
    pub fn play_stream(&self, stream: StreamId) {
        unimplemented!()
    }
    
    pub fn pause_stream(&self, stream: StreamId) {
        unimplemented!()
    }
    pub fn destroy_stream(&self, stream_id: StreamId) {
        unimplemented!()
    }
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData)
        {
            unimplemented!()
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

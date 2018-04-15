extern crate asio_sys as sys;

use Format;
use CreationError;
use StreamData;
use std::marker::PhantomData;
use super::Device;
use::std::cell::Cell;
use::std::cell::RefCell;
use UnknownTypeOutputBuffer;
use std::sync::Mutex;


pub struct EventLoop{
    asio_stream: RefCell<Option<sys::AsioStream>>,
    stream_count: Cell<usize>,
    callbacks: Mutex<Vec<Box<FnMut(StreamId, StreamData)>>>,
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
        EventLoop{ asio_stream: RefCell::new(None),
        stream_count: Cell::new(0),
        callbacks: Mutex::new(Vec::new())}
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
                    *self.asio_stream.borrow_mut() = Some(stream);
                }
                let count = self.stream_count.get();
                self.stream_count.set(count + 1);
                if let Some(asio_stream) = *self.asio_stream.borrow() {
                    sys::set_callback(move |index| {
                        let buff = OutputBuffer{
                            buffer: asio_stream.buffer_info.buffers[index as usize]
                        };
                        let callbacks = self.callbacks.lock().unwrap();
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
                    });
                }
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
        let old_stream = self.asio_stream.replace(None);
        sys::destroy_stream(old_stream.unwrap());
    }
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData)
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

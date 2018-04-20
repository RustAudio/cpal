extern crate asio_sys as sys;
extern crate itertools;

use std;
use Format;
use CreationError;
use StreamData;
use std::marker::PhantomData;
use super::Device;
use::std::cell::Cell;
use UnknownTypeOutputBuffer;
use std::sync::{Mutex, Arc};
use std::mem;
use self::itertools::Itertools;


pub struct EventLoop{
    asio_stream: Arc<Mutex<Option<sys::AsioStream>>>,
    stream_count: Cell<usize>,
    callbacks: Arc<Mutex<Vec<&'static mut (FnMut(StreamId, StreamData) + Send)>>>,
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
                let bytes_per_channel = format.data_type.sample_size();
                let num_channels = format.channels.clone();

                sys::set_callback(move |index| unsafe{
                    if let Some(ref asio_stream) = *asio_stream
                        .lock().unwrap(){
                            let cpal_num_samples = (asio_stream.buffer_size as usize) *  num_channels as usize;
                            /*
                            let data_slice = std::slice::from_raw_parts_mut(
                                asio_stream.buffer_info.buffers[index as usize] as *mut i16,
                                data_len);
                                */
                            let mut callbacks = callbacks.lock().unwrap();
                            match callbacks.first_mut(){
                                Some(callback) => {
                                    let mut cpal_buffer = vec![0i16; cpal_num_samples];
                                    {
                                        let buff = OutputBuffer{
                                            buffer: &mut cpal_buffer 
                                        };
                                        callback(
                                            StreamId(count),
                                            StreamData::Output{ 
                                                buffer: UnknownTypeOutputBuffer::I16(
                                                            ::OutputBuffer{ 
                                                                target: Some(super::super::OutputBuffer::Asio(buff))
                                                            })
                                            }
                                            ); 
                                    }
                                    fn deinterleave(data_slice: &mut [i16]) -> (Vec<i16>,
                                                                                Vec<i16>){
                                        let mut first: Vec<i16> = data_slice.iter().cloned().step(2).collect();
                                        let mut it = data_slice.iter().cloned();
                                        it.next();
                                        let mut second: Vec<i16> = it.step(2).collect();
                                        (first, second)
                                    }
                                    let (cpal_left,
                                         cpal_right) = deinterleave(&mut cpal_buffer[..]);
                                   
                                    let left_buff_ptr = (asio_stream.buffer_infos[0].buffers[index as usize] as *mut i32);
                                    let right_buff_ptr = left_buff_ptr.offset(asio_stream.buffer_size as isize);
                                    println!("left: {:?}", left_buff_ptr);
                                    println!("right: {:?}", right_buff_ptr);
                                    /*
                                    let right_buff_ptr = asio_stream.buffer_infos[1].buffers[index as usize] as *mut i32;
                                    */
                                    let asio_buffer_left: &'static mut [i32] = std::slice::from_raw_parts_mut(left_buff_ptr, asio_stream.buffer_size as usize);
                                    
                                    let asio_buffer_right: &'static mut [i32] = std::slice::from_raw_parts_mut(right_buff_ptr, asio_stream.buffer_size as usize);
                                    for (asio_s, cpal_s) in asio_buffer_left.iter_mut()
                                        .zip(&cpal_left){
                                        *asio_s = (*cpal_s as i64 * ::std::i32::MAX as i64 /
                                                   ::std::i16::MAX as i64) as i32;
                                    }
                                    for (asio_s, cpal_s) in asio_buffer_right.iter_mut()
                                        .zip(&cpal_right){
                                        *asio_s = (*cpal_s as i64 * ::std::i32::MAX as i64 /
                                                   ::std::i16::MAX as i64) as i32;
                                    }

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
        let mut asio_stream_lock = self.asio_stream.lock().unwrap();
        let old_stream = mem::replace(&mut *asio_stream_lock, None);
        if let Some(old_stream) = old_stream{
            sys::destroy_stream(old_stream);
        }
    }
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData) + Send
        {
            let callback: &mut (FnMut(StreamId, StreamData) + Send) = &mut callback;
            self.callbacks
                .lock()
                .unwrap()
                .push(unsafe{ mem::transmute(callback) });
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
        &mut self.buffer
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn finish(self) {
    }
}

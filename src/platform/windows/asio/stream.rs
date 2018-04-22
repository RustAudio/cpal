extern crate asio_sys as sys;
extern crate itertools;

use std;
use Format;
use CreationError;
use StreamData;
use std::marker::PhantomData;
use super::Device;
use std::cell::Cell;
use UnknownTypeOutputBuffer;
use std::sync::{Arc, Mutex};
use std::mem;
use self::itertools::Itertools;

pub struct EventLoop {
    asio_stream: Arc<Mutex<Option<sys::AsioStream>>>,
    stream_count: Cell<usize>,
    callbacks: Arc<Mutex<Vec<&'static mut (FnMut(StreamId, StreamData) + Send)>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

pub struct InputBuffer<'a, T: 'a> {
    marker: PhantomData<&'a T>,
}
pub struct OutputBuffer<'a, T: 'a> {
    buffer: &'a mut [T],
}

impl EventLoop {
    pub fn new() -> EventLoop {
        EventLoop {
            asio_stream: Arc::new(Mutex::new(None)),
            stream_count: Cell::new(0),
            callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError> {
        /*
        let stream_type = sys::get_data_type(&device.driver_name).expect("Couldn't load data type");
        match sys::prepare_stream(&device.driver_name) {
            Ok(stream) => {
                {
                    *self.asio_stream.lock().unwrap() = Some(stream);
                }
                let count = self.stream_count.get();
                self.stream_count.set(count + 1);
                let asio_stream = self.asio_stream.clone();
                let callbacks = self.callbacks.clone();
                let bytes_per_channel = format.data_type.sample_size();
                let num_channels = format.channels.clone();

                // Get stream types

                sys::set_callback(move |index| unsafe {
                    if let Some(ref asio_stream) = *asio_stream.lock().unwrap() {
                        // Number of samples needed total
                        let cpal_num_samples =
                            (asio_stream.buffer_size as usize) * num_channels as usize;
                        let mut callbacks = callbacks.lock().unwrap();

                        // Assuming only one callback, probably needs to change
                        match callbacks.first_mut() {
                            Some(callback) => {
                                macro_rules! try_callback {
                                        ($SampleFormat:ident,
                                         $SampleType:ty,
                                         $SampleTypeIdent:ident,
                                         $AsioType:ty,
                                         $AsioTypeIdent:ident) => {
                                            // Buffer that is filled by cpal.
                                            let mut cpal_buffer: Vec<$SampleType> = vec![0 as $SampleType; cpal_num_samples];
                                            //  Call in block because of mut borrow
                                            {
                                            }
                                            // Function for deinterleaving because
                                            // cpal writes to buffer interleaved
                                            fn interleave(channels: Vec<Vec<$SampleType>>) -> Vec<$SampleType>{
                                                let mut buffer: Vec<$SampleType> = Vec::new();
                                                let length = channels[0].len();
                                                for i in 0..length{
                                                    for channel in channels{
                                                        buffer.push(channel[i]);
                                                    }
                                                }
                                                buffer
                                            }
                                            // Deinter all the channels
                                            let deinter_channels = deinterleave(&mut cpal_buffer[..],
                                                                                num_channels as usize);

                                            // For each channel write the cpal data to
                                            // the asio buffer
                                            // Also need to check for Endian
                                            for (i, channel) in deinter_channels.into_iter().enumerate(){
                                                let buff_ptr = (asio_stream
                                                                .buffer_infos[i]
                                                                .buffers[index as usize] as *mut $AsioType)
                                                    .offset(asio_stream.buffer_size as isize * i as isize);
                                                let asio_buffer: &'static mut [$AsioType] =
                                                    std::slice::from_raw_parts_mut(
                                                        buff_ptr,
                                                        asio_stream.buffer_size as usize);
                                                for (asio_s, cpal_s) in asio_buffer.iter_mut()
                                                    .zip(&channel){
                                                        *asio_s = (*cpal_s as i64 *
                                                                   ::std::$AsioTypeIdent::MAX as i64 /
                                                                   ::std::$SampleTypeIdent::MAX as i64) as $AsioType;
                                                    }

                                            }


                                            let buff = InputBuffer{
                                                buffer: &mut cpal_buffer
                                            };
                                            callback(
                                                StreamId(count),
                                                StreamData::Input{
                                                    buffer: UnknownTypeInputBuffer::$SampleFormat(
                                                                ::InputBuffer{
                                                                    target: Some(super::super::InputBuffer::Asio(buff))
                                                                })
                                                }
                                                );
                                        };
                                    }
                                // Generic over types
                                // TODO check for endianess
                                match stream_type {
                                    sys::AsioSampleType::ASIOSTInt32LSB => {
                                        try_callback!(I16, i16, i16, i32, i32);
                                    }
                                    sys::AsioSampleType::ASIOSTInt16LSB => {
                                        try_callback!(I16, i16, i16, i16, i16);
                                    }
                                    sys::AsioSampleType::ASIOSTFloat32LSB => {
                                        try_callback!(F32, f32, f32, f32, f32);
                                    }
                                    sys::AsioSampleType::ASIOSTFloat32LSB => {
                                        try_callback!(F32, f32, f32, f64, f64);
                                    }
                                    _ => println!("unsupported format {:?}", stream_type),
                                }
                            }
                            None => return (),
                        }
                    }
                });
                Ok(StreamId(count))
            }
            Err(ref e) => {
                println!("Error preparing stream: {}", e);
                Err(CreationError::DeviceNotAvailable)
            }
        }
    */
        unimplemented!()
    }

    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError> {
        let stream_type = sys::get_data_type(&device.driver_name).expect("Couldn't load data type");
        match sys::prepare_stream(&device.driver_name) {
            Ok(stream) => {
                {
                    *self.asio_stream.lock().unwrap() = Some(stream);
                }
                let count = self.stream_count.get();
                self.stream_count.set(count + 1);
                let asio_stream = self.asio_stream.clone();
                let callbacks = self.callbacks.clone();
                let bytes_per_channel = format.data_type.sample_size();
                let num_channels = format.channels.clone();

                // Get stream types

                sys::set_callback(move |index| unsafe {
                    if let Some(ref asio_stream) = *asio_stream.lock().unwrap() {
                        // Number of samples needed total
                        let cpal_num_samples =
                            (asio_stream.buffer_size as usize) * num_channels as usize;
                        let mut callbacks = callbacks.lock().unwrap();

                        // Assuming only one callback, probably needs to change
                        match callbacks.first_mut() {
                            Some(callback) => {
                                macro_rules! try_callback {
                                        ($SampleFormat:ident,
                                         $SampleType:ty,
                                         $SampleTypeIdent:ident,
                                         $AsioType:ty,
                                         $AsioTypeIdent:ident) => {
                                            // Buffer that is filled by cpal.
                                            let mut cpal_buffer: Vec<$SampleType> = vec![0 as $SampleType; cpal_num_samples];
                                            //  Call in block because of mut borrow
                                            {
                                                let buff = OutputBuffer{
                                                    buffer: &mut cpal_buffer
                                                };
                                                callback(
                                                    StreamId(count),
                                                    StreamData::Output{
                                                        buffer: UnknownTypeOutputBuffer::$SampleFormat(
                                                                    ::OutputBuffer{
                                                                        target: Some(super::super::OutputBuffer::Asio(buff))
                                                                    })
                                                    }
                                                    );
                                            }
                                            // Function for deinterleaving because
                                            // cpal writes to buffer interleaved
                                            fn deinterleave(data_slice: &mut [$SampleType],
                                                            num_channels: usize) -> Vec<Vec<$SampleType>>{
                                                let mut channels: Vec<Vec<$SampleType>> = Vec::new();
                                                for i in 0..num_channels{
                                                    let mut it = data_slice.iter().skip(i).cloned();
                                                    let channel = it.step(num_channels).collect();
                                                    channels.push(channel);
                                                }
                                                channels
                                            }
                                            // Deinter all the channels
                                            let deinter_channels = deinterleave(&mut cpal_buffer[..],
                                                                                num_channels as usize);

                                            // For each channel write the cpal data to
                                            // the asio buffer
                                            // Also need to check for Endian
                                            for (i, channel) in deinter_channels.into_iter().enumerate(){
                                                let buff_ptr = (asio_stream
                                                                .buffer_infos[i]
                                                                .buffers[index as usize] as *mut $AsioType)
                                                    .offset(asio_stream.buffer_size as isize * i as isize);
                                                let asio_buffer: &'static mut [$AsioType] =
                                                    std::slice::from_raw_parts_mut(
                                                        buff_ptr,
                                                        asio_stream.buffer_size as usize);
                                                for (asio_s, cpal_s) in asio_buffer.iter_mut()
                                                    .zip(&channel){
                                                        *asio_s = (*cpal_s as i64 *
                                                                   ::std::$AsioTypeIdent::MAX as i64 /
                                                                   ::std::$SampleTypeIdent::MAX as i64) as $AsioType;
                                                    }

                                            }
                                        };
                                    }
                                // Generic over types
                                // TODO check for endianess
                                match stream_type {
                                    sys::AsioSampleType::ASIOSTInt32LSB => {
                                        try_callback!(I16, i16, i16, i32, i32);
                                    }
                                    sys::AsioSampleType::ASIOSTInt16LSB => {
                                        try_callback!(I16, i16, i16, i16, i16);
                                    }
                                    sys::AsioSampleType::ASIOSTFloat32LSB => {
                                        try_callback!(F32, f32, f32, f32, f32);
                                    }
                                    sys::AsioSampleType::ASIOSTFloat32LSB => {
                                        try_callback!(F32, f32, f32, f64, f64);
                                    }
                                    _ => println!("unsupported format {:?}", stream_type),
                                }
                            }
                            None => return (),
                        }
                    }
                });
                Ok(StreamId(count))
            }
            Err(ref e) => {
                println!("Error preparing stream: {}", e);
                Err(CreationError::DeviceNotAvailable)
            }
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
        if let Some(old_stream) = old_stream {
            sys::destroy_stream(old_stream);
        }
    }
    pub fn run<F>(&self, mut callback: F) -> !
    where
        F: FnMut(StreamId, StreamData) + Send,
    {
        let callback: &mut (FnMut(StreamId, StreamData) + Send) = &mut callback;
        self.callbacks
            .lock()
            .unwrap()
            .push(unsafe { mem::transmute(callback) });
        loop {
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

    pub fn finish(self) {}
}

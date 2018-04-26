extern crate asio_sys as sys;

use std;
use Format;
use CreationError;
use StreamData;
use std::marker::PhantomData;
use super::Device;
use std::cell::Cell;
use UnknownTypeOutputBuffer;
use UnknownTypeInputBuffer;
use std::sync::{Arc, Mutex};
use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};
use SampleFormat;

pub struct EventLoop {
    asio_stream: Arc<Mutex<Option<sys::AsioStream>>>,
    stream_count: Arc<AtomicUsize>,
    callbacks: Arc<Mutex<Vec<&'static mut (FnMut(StreamId, StreamData) + Send)>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

pub struct InputBuffer<'a, T: 'a> {
    buffer: &'a [T],
}
pub struct OutputBuffer<'a, T: 'a> {
    buffer: &'a mut [T],
}

impl EventLoop {
    pub fn new() -> EventLoop {
        EventLoop {
            asio_stream: Arc::new(Mutex::new(None)),
            stream_count: Arc::new(AtomicUsize::new(0)),
            callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
        ) -> Result<StreamId, CreationError> {
        let stream_type = sys::get_data_type(&device.driver_name).expect("Couldn't load data type");
        match sys::prepare_input_stream(&device.driver_name) {
            Ok(stream) => {
                {
                    *self.asio_stream.lock().unwrap() = Some(stream);
                }
                let count = self.stream_count.load(Ordering::SeqCst);
                self.stream_count.store(count + 1, Ordering::SeqCst);
                let asio_stream = self.asio_stream.clone();
                let callbacks = self.callbacks.clone();
                let bytes_per_channel = format.data_type.sample_size();
                let num_channels = format.channels.clone();
                
                // Create buffers 
                let cpal_num_samples =
                    (stream.buffer_size as usize) * num_channels as usize;
                let channel_len = cpal_num_samples 
                    / num_channels as usize;

                enum CpalBuffer{
                    I16(Vec<i16>),
                    U16(Vec<u16>),
                    F32(Vec<f32>),
                }
                enum ChannelBuffer{
                    I16(Vec<Vec<i16>>),
                    U16(Vec<Vec<u16>>),
                    F32(Vec<Vec<f32>>),
                }
                let (mut cpal_buffer,
                 mut channels) = match format.data_type{
                    SampleFormat::I16 => {
                        let mut cpal_buffer = CpalBuffer::I16(vec![0 as i16; cpal_num_samples]);
                        let mut channels = ChannelBuffer::I16(
                            (0..num_channels)
                            .map(|_| Vec::with_capacity(channel_len))
                            .collect());
                        (cpal_buffer, channels)
                    }
                    SampleFormat::U16 => {
                        let mut cpal_buffer = CpalBuffer::U16(vec![0 as u16; cpal_num_samples]);
                        let mut channels = ChannelBuffer::U16(
                            (0..num_channels)
                            .map(|_| Vec::with_capacity(channel_len))
                            .collect());
                        (cpal_buffer, channels)
                    }
                    SampleFormat::F32 => {
                        let mut cpal_buffer = CpalBuffer::F32(vec![0 as f32; cpal_num_samples]);
                        let mut channels = ChannelBuffer::F32(
                            (0..num_channels)
                            .map(|_| Vec::with_capacity(channel_len))
                            .collect());
                        (cpal_buffer, channels)
                    }
                };

                sys::set_callback(move |index| unsafe {
                    if let Some(ref asio_stream) = *asio_stream.lock().unwrap() {
                        // Number of samples needed total
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
                                        // Function for deinterleaving because
                                        // cpal writes to buffer interleaved
                                        fn interleave(channels: &[Vec<$SampleType>],
                                                      buffer: &mut Vec<$SampleType>) {
                                            let length = channels[0].len();
                                            for i in 0..length{
                                                for channel in channels{
                                                    buffer.push(channel[i]);
                                                }
                                            }
                                        }

                                        // For each channel write the cpal data to
                                        // the asio buffer
                                        // Also need to check for Endian

                                        for (i, channel) in channels.iter_mut().enumerate(){
                                            let buff_ptr = asio_stream
                                                            .buffer_infos[i]
                                                            .buffers[index as usize] as *mut $AsioType;
                                                //.offset(asio_stream.buffer_size as isize * i as isize);
                                            let asio_buffer: &'static [$AsioType] =
                                                std::slice::from_raw_parts(
                                                    buff_ptr,
                                                    asio_stream.buffer_size as usize);
                                            for asio_s in asio_buffer.iter(){
                                                channel.push( (*asio_s as i64 *
                                                               ::std::$SampleTypeIdent::MAX as i64 /
                                                               ::std::$AsioTypeIdent::MAX as i64) as $SampleType);
                                            }
                                        }


                                        // interleave all the channels
                                        interleave(&channels, &mut cpal_buffer);


                                        let buff = InputBuffer{
                                            buffer: &mut cpal_buffer 
                                        };
                                        callback(
                                            StreamId(count),
                                            StreamData::Input{
                                                buffer: UnknownTypeInputBuffer::$SampleFormat(
                                                            ::InputBuffer{
                                                                buffer: Some(super::super::InputBuffer::Asio(buff))
                                                            })
                                            }
                                            );
                                    }
                                };
                                // Generic over types
                                // TODO check for endianess
                                match (stream_type, cpal_buffer, channels) {
                                    (sys::AsioSampleType::ASIOSTInt32LSB,
                                     CpalBuffer::I16(cpal_buffer),
                                     ChannelBuffer::I16(channels))=> {
                                        try_callback!(I16, i16, i16, i32, i32);
                                    }
                                    (sys::AsioSampleType::ASIOSTInt16LSB,
                                     CpalBuffer::I16(cpal_buffer),
                                     ChannelBuffer::I16(channels))=> {
                                        try_callback!(I16, i16, i16, i16, i16);
                                    }
                                    (sys::AsioSampleType::ASIOSTFloat32LSB,
                                     CpalBuffer::F32(cpal_buffer),
                                     ChannelBuffer::F32(channels))=> {
                                        try_callback!(F32, f32, f32, f32, f32);
                                    }
                                    (sys::AsioSampleType::ASIOSTFloat64LSB,
                                     CpalBuffer::F32(cpal_buffer),
                                     ChannelBuffer::F32(channels))=> {
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
            let count = self.stream_count.load(Ordering::SeqCst);
            self.stream_count.store(count + 1, Ordering::SeqCst);
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
                                                    num_channels: usize,
                                                    channels: &mut [Vec<$SampleType>]) {
                                        for (i, &sample) in data_slice.iter().enumerate() {
                                            let ch = i % num_channels;
                                            channels[ch].push(sample);
                                        }
                                    }
                                    // Deinter all the channels
                                    let channel_len = cpal_buffer.len() 
                                        / num_channels as usize;
                                    let mut deinter_channels: Vec<_> = (0..num_channels)
                                        .map(|_| Vec::with_capacity(channel_len))
                                        .collect();
                                    deinterleave(&mut cpal_buffer[..],
                                                 num_channels as usize,
                                                 &mut deinter_channels);

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
                                sys::AsioSampleType::ASIOSTFloat64LSB => {
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
        &self.buffer
    }
    pub fn finish(self) {
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

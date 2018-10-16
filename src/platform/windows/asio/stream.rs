extern crate asio_sys as sys;
extern crate asio_utils as au;

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

#[derive(Default)]
struct I16Buffer{
    cpal: Vec<i16>,
    channel: Vec<Vec<i16>>,
}
#[derive(Default)]
struct U16Buffer{
    cpal: Vec<u16>,
    channel: Vec<Vec<u16>>,
}
#[derive(Default)]
struct F32Buffer{
    cpal: Vec<f32>,
    channel: Vec<Vec<f32>>,
}
struct Buffers {
    i16_buff: I16Buffer,
    u16_buff: U16Buffer,
    f32_buff: F32Buffer,
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
            let Device {
                drivers,
                ..
            } = device;
        let stream_type = drivers.get_data_type().expect("Couldn't load data type");
        match drivers.prepare_input_stream() {
            Ok(stream) => {
                let num_channels = format.channels.clone();
                let cpal_num_samples =
                    (stream.buffer_size as usize) * num_channels as usize;
                {
                    *self.asio_stream.lock().unwrap() = Some(stream);
                }
                let count = self.stream_count.load(Ordering::SeqCst);
                self.stream_count.store(count + 1, Ordering::SeqCst);
                let asio_stream = self.asio_stream.clone();
                let callbacks = self.callbacks.clone();
                let bytes_per_channel = format.data_type.sample_size();
                
                // Create buffers 
                let channel_len = cpal_num_samples 
                    / num_channels as usize;
                
                
                let mut buffers = match format.data_type{
                    SampleFormat::I16 => {
                        Buffers{
                            i16_buff: I16Buffer{
                            cpal: vec![0 as i16; cpal_num_samples],
                            channel: (0..num_channels)
                            .map(|_| Vec::with_capacity(channel_len))
                            .collect()},
                            u16_buff: U16Buffer::default(),
                            f32_buff: F32Buffer::default(),
                        }
                    }
                    SampleFormat::U16 => {
                        Buffers{
                            i16_buff: I16Buffer::default(),
                            u16_buff: U16Buffer{
                            cpal: vec![0 as u16; cpal_num_samples],
                            channel: (0..num_channels)
                            .map(|_| Vec::with_capacity(channel_len))
                            .collect()},
                            f32_buff: F32Buffer::default(),
                        }
                    }
                    SampleFormat::F32 => {
                        Buffers{
                            i16_buff: I16Buffer::default(),
                            u16_buff: U16Buffer::default(),
                            f32_buff: F32Buffer{
                            cpal: vec![0 as f32; cpal_num_samples],
                            channel: (0..num_channels)
                            .map(|_| Vec::with_capacity(channel_len))
                            .collect()},
                        }
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
                                     $AsioTypeIdent:ident,
                                     $Buffers:expr,
                                     $BuffersType:ty,
                                     $BuffersTypeIdent:ident
                                     ) => {

                                        // For each channel write the cpal data to
                                        // the asio buffer
                                        // Also need to check for Endian

                                        for (i, channel) in $Buffers.channel.iter_mut().enumerate(){
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
                                        {
                                            let $BuffersTypeIdent {
                                                cpal: ref mut c_buffer,
                                                channel: ref mut channels,
                                            } = $Buffers;
                                            au::interleave(&channels, c_buffer);
                                            for c in channels.iter_mut() {
                                                c.clear();
                                            }
                                        }


                                        let buff = InputBuffer{
                                            buffer: &mut $Buffers.cpal,
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
                                match stream_type {
                                    sys::AsioSampleType::ASIOSTInt32LSB => {
                                        try_callback!(I16, i16, i16, i32, i32, 
                                        buffers.i16_buff, I16Buffer, I16Buffer);
                                    }
                                    sys::AsioSampleType::ASIOSTInt16LSB => {
                                        try_callback!(I16, i16, i16, i16, i16, 
                                        buffers.i16_buff, I16Buffer, I16Buffer);
                                    }
                                    sys::AsioSampleType::ASIOSTFloat32LSB => {
                                        try_callback!(F32, f32, f32, f32, f32, 
                                        buffers.f32_buff, F32Buffer, F32Buffer);
                                    }
                                    sys::AsioSampleType::ASIOSTFloat64LSB => {
                                        try_callback!(F32, f32, f32, f64, f64, 
                                        buffers.f32_buff, F32Buffer, F32Buffer);
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
    let Device {
        drivers,
        ..
    } = device;
    let stream_type = drivers.get_data_type().expect("Couldn't load data type");
    match drivers.prepare_output_stream() {
        Ok(stream) => {
            let num_channels = format.channels.clone();

            let cpal_num_samples =
                (stream.buffer_size as usize) * num_channels as usize;
            {
                *self.asio_stream.lock().unwrap() = Some(stream);
            }
            let count = self.stream_count.load(Ordering::SeqCst);
            self.stream_count.store(count + 1, Ordering::SeqCst);
            let asio_stream = self.asio_stream.clone();
            let callbacks = self.callbacks.clone();
            let bytes_per_channel = format.data_type.sample_size();
            // Create buffers 
            let channel_len = cpal_num_samples 
                / num_channels as usize;
            
            
            let mut re_buffers = match format.data_type{
                SampleFormat::I16 => {
                    Buffers{
                        i16_buff: I16Buffer{
                        cpal: vec![0 as i16; cpal_num_samples],
                        channel: (0..num_channels)
                        .map(|_| Vec::with_capacity(channel_len))
                        .collect()},
                        u16_buff: U16Buffer::default(),
                        f32_buff: F32Buffer::default(),
                    }
                }
                SampleFormat::U16 => {
                    Buffers{
                        i16_buff: I16Buffer::default(),
                        u16_buff: U16Buffer{
                        cpal: vec![0 as u16; cpal_num_samples],
                        channel: (0..num_channels)
                        .map(|_| Vec::with_capacity(channel_len))
                        .collect()},
                        f32_buff: F32Buffer::default(),
                    }
                }
                SampleFormat::F32 => {
                    Buffers{
                        i16_buff: I16Buffer::default(),
                        u16_buff: U16Buffer::default(),
                        f32_buff: F32Buffer{
                        cpal: vec![0 as f32; cpal_num_samples],
                        channel: (0..num_channels)
                        .map(|_| Vec::with_capacity(channel_len))
                        .collect()},
                    }
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
                                 $AsioTypeIdent:ident,
                                 $Buffers:expr,
                                 $BuffersType:ty,
                                 $BuffersTypeIdent:ident
                                 ) => {
                                     let mut my_buffers = $Buffers;
                                    // Buffer that is filled by cpal.
                                    //let mut cpal_buffer: Vec<$SampleType> = vec![0 as $SampleType; cpal_num_samples];
                                    //  Call in block because of mut borrow
                                    {
                                        let buff = OutputBuffer{
                                            buffer: &mut my_buffers.cpal 
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
                                    // Deinter all the channels
                                    {
                                        let $BuffersTypeIdent {
                                            cpal: ref mut c_buffer,
                                            channel: ref mut channels,
                                        } = my_buffers;
                                        au::deinterleave(&c_buffer[..], channels);
                                    }

                                    // For each channel write the cpal data to
                                    // the asio buffer
                                    // Also need to check for Endian
                                    for (i, channel) in my_buffers.channel.iter().enumerate(){
                                        let buff_ptr = (asio_stream
                                                        .buffer_infos[i]
                                                        .buffers[index as usize] as *mut $AsioType)
                                            .offset(asio_stream.buffer_size as isize * i as isize);
                                        let asio_buffer: &'static mut [$AsioType] =
                                            std::slice::from_raw_parts_mut(
                                                buff_ptr,
                                                asio_stream.buffer_size as usize);
                                        for (asio_s, cpal_s) in asio_buffer.iter_mut()
                                            .zip(channel){
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
                                    try_callback!(I16, i16, i16, i32, i32, 
                                    &mut re_buffers.i16_buff, I16Buffer, I16Buffer);
                                }
                                sys::AsioSampleType::ASIOSTInt16LSB => {
                                    try_callback!(I16, i16, i16, i16, i16, 
                                    &mut re_buffers.i16_buff, I16Buffer, I16Buffer);
                                }
                                sys::AsioSampleType::ASIOSTFloat32LSB => {
                                    try_callback!(F32, f32, f32, f32, f32, 
                                    &mut re_buffers.f32_buff, F32Buffer, F32Buffer);
                                }
                                sys::AsioSampleType::ASIOSTFloat64LSB => {
                                    try_callback!(F32, f32, f32, f64, f64, 
                                    &mut re_buffers.f32_buff, F32Buffer, F32Buffer);
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

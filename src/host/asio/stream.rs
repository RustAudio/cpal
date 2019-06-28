extern crate asio_sys as sys;
extern crate num_traits;

use self::num_traits::PrimInt;
use super::asio_utils as au;
use super::Device;
use std;
use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use BackendSpecificError;
use BuildStreamError;
use Format;
use PauseStreamError;
use PlayStreamError;
use SampleFormat;
use StreamData;
use StreamDataResult;
use UnknownTypeInputBuffer;
use UnknownTypeOutputBuffer;

/// Sample types whose constant silent value is known.
trait Silence {
    const SILENCE: Self;
}

/// Constraints on the interleaved sample buffer format required by the CPAL API.
trait InterleavedSample: Clone + Copy + Silence {
    fn unknown_type_output_buffer(&mut [Self]) -> UnknownTypeOutputBuffer;
}

/// Constraints on the ASIO sample types.
trait AsioSample: Clone + Copy + Silence + std::ops::Add<Self, Output = Self> {}

/// Controls all streams
pub struct EventLoop {
    /// The input and output ASIO streams
    asio_streams: Arc<Mutex<sys::AsioStreams>>,
    /// List of all CPAL streams
    cpal_streams: Arc<Mutex<Vec<Option<Stream>>>>,
    /// Total stream count
    stream_count: AtomicUsize,
    /// The CPAL callback that the user gives to fill the buffers.
    callbacks: Arc<Mutex<Option<&'static mut (FnMut(StreamId, StreamDataResult) + Send)>>>,
}

/// Id for each stream.
/// Created depending on the number they are created.
/// Starting at one! not zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

/// CPAL stream.
/// This decouples the many cpal streams
/// from the single input and single output
/// ASIO streams.
/// Each stream can be playing or paused.
struct Stream {
    playing: bool,
    // The driver associated with this stream.
    driver: Arc<sys::Driver>,
}

struct Buffers {
    interleaved: Vec<u8>,
    non_interleaved: Vec<u8>,
}

enum Endian {
    Little,
    Big,
}

// Used to keep track of whether or not the current current asio stream buffer requires
// being silencing before summing audio.
#[derive(Default)]
struct SilenceAsioBuffer {
    first: bool,
    second: bool,
}

impl EventLoop {
    pub fn new() -> EventLoop {
        EventLoop {
            asio_streams: Arc::new(Mutex::new(sys::AsioStreams {
                input: None,
                output: None,
            })),
            cpal_streams: Arc::new(Mutex::new(Vec::new())),
            // This is why the Id's count from one not zero
            // because at this point there is no streams
            stream_count: AtomicUsize::new(0),
            callbacks: Arc::new(Mutex::new(None)),
        }
    }

    fn check_format(
        &self,
        driver: &sys::Driver,
        format: &Format,
        num_asio_channels: u16,
    ) -> Result<(), BuildStreamError> {
        let Format {
            channels,
            sample_rate,
            data_type,
        } = format;
        // Try and set the sample rate to what the user selected.
        let sample_rate = sample_rate.0.into();
        if sample_rate != driver.sample_rate().map_err(build_stream_err)? {
            if driver.can_sample_rate(sample_rate).map_err(build_stream_err)? {
                driver
                    .set_sample_rate(sample_rate)
                    .map_err(build_stream_err)?;
            } else {
                return Err(BuildStreamError::FormatNotSupported);
            }
        }
        // unsigned formats are not supported by asio
        match data_type {
            SampleFormat::I16 | SampleFormat::F32 => (),
            SampleFormat::U16 => return Err(BuildStreamError::FormatNotSupported),
        }
        if *channels > num_asio_channels {
            return Err(BuildStreamError::FormatNotSupported);
        }
        Ok(())
    }

    /// Create a new CPAL Input Stream.
    /// If there is no ASIO Input Stream
    /// it will be created.
    fn get_input_stream(
        &self,
        driver: &sys::Driver,
        format: &Format,
        device: &Device,
    ) -> Result<usize, BuildStreamError> {
        match device.default_input_format() {
            Ok(f) => {
                let num_asio_channels = f.channels;
                self.check_format(driver, format, num_asio_channels)
            },
            Err(_) => Err(BuildStreamError::FormatNotSupported),
        }?;
        let num_channels = format.channels as usize;
        let ref mut streams = *self.asio_streams.lock().unwrap();
        // Either create a stream if thers none or had back the
        // size of the current one.
        match streams.input {
            Some(ref input) => Ok(input.buffer_size as usize),
            None => {
                let output = streams.output.take();
                driver
                    .prepare_input_stream(output, num_channels)
                    .map(|new_streams| {
                        let bs = match new_streams.input {
                            Some(ref inp) => inp.buffer_size as usize,
                            None => unreachable!(),
                        };
                        *streams = new_streams;
                        bs
                    }).map_err(|ref e| {
                        println!("Error preparing stream: {}", e);
                        BuildStreamError::DeviceNotAvailable
                    })
            }
        }
    }

    /// Create a new CPAL Output Stream.
    /// If there is no ASIO Output Stream
    /// it will be created.
    fn get_output_stream(
        &self,
        driver: &sys::Driver,
        format: &Format,
        device: &Device,
    ) -> Result<usize, BuildStreamError> {
        match device.default_output_format() {
            Ok(f) => {
                let num_asio_channels = f.channels;
                self.check_format(driver, format, num_asio_channels)
            },
            Err(_) => Err(BuildStreamError::FormatNotSupported),
        }?;
        let num_channels = format.channels as usize;
        let ref mut streams = *self.asio_streams.lock().unwrap();
        // Either create a stream if thers none or had back the
        // size of the current one.
        match streams.output {
            Some(ref output) => Ok(output.buffer_size as usize),
            None => {
                let input = streams.input.take();
                driver
                    .prepare_output_stream(input, num_channels)
                    .map(|new_streams| {
                        let bs = match new_streams.output {
                            Some(ref out) => out.buffer_size as usize,
                            None => unreachable!(),
                        };
                        *streams = new_streams;
                        bs
                    }).map_err(|ref e| {
                        println!("Error preparing stream: {}", e);
                        BuildStreamError::DeviceNotAvailable
                    })
            }
        }
    }

    /// Builds a new cpal input stream
    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError> {
        unimplemented!()
        // let Device { driver, .. } = device;
        // let num_channels = format.channels.clone();
        // let stream_type = driver.data_type().map_err(build_stream_err)?;
        // let stream_buffer_size = self.get_input_stream(&driver, format, device)?;
        // let cpal_num_samples = stream_buffer_size * num_channels as usize;
        // let count = self.stream_count.fetch_add(1, Ordering::SeqCst);
        // let asio_streams = self.asio_streams.clone();
        // let cpal_streams = self.cpal_streams.clone();
        // let callbacks = self.callbacks.clone();

        // let channel_len = cpal_num_samples / num_channels as usize;

        // // Create buffers depending on data type
        // // TODO the naming of cpal and channel is confusing.
        // // change it to:
        // // cpal -> interleaved
        // // channels -> per_channel
        // let mut buffers = match format.data_type {
        //     SampleFormat::I16 => Buffers {
        //         i16_buff: I16Buffer {
        //             cpal: vec![0 as i16; cpal_num_samples],
        //             channel: (0..num_channels)
        //                 .map(|_| Vec::with_capacity(channel_len))
        //                 .collect(),
        //         },
        //         f32_buff: F32Buffer::default(),
        //     },
        //     SampleFormat::F32 => Buffers {
        //         i16_buff: I16Buffer::default(),
        //         f32_buff: F32Buffer {
        //             cpal: vec![0 as f32; cpal_num_samples],
        //             channel: (0..num_channels)
        //                 .map(|_| Vec::with_capacity(channel_len))
        //                 .collect(),
        //         },
        //     },
        //     _ => unimplemented!(),
        // };

        // // Set the input callback.
        // // This is most performance critical part of the ASIO bindings.
        // sys::set_callback(move |index| unsafe {
        //     // if not playing return early
        //     {
        //         if let Some(s) = cpal_streams.lock().unwrap().get(count) {
        //             if let Some(s) = s {
        //                 if !s.playing {
        //                     return ();
        //                 }
        //             }
        //         }
        //     }
        //     // Get the stream
        //     let stream_lock = asio_streams.lock().unwrap();
        //     let ref asio_stream = match stream_lock.input {
        //         Some(ref asio_stream) => asio_stream,
        //         None => return (),
        //     };

        //     // Get the callback
        //     let mut callbacks = callbacks.lock().unwrap();

        //     // Theres only a single callback because theres only one event loop
        //     let callback = match callbacks.as_mut() {
        //         Some(callback) => callback,
        //         None => return (),
        //     };

        //     // Macro to convert sample from ASIO to CPAL type
        //     macro_rules! convert_sample {
        //         // floats types required different conversion
        //         (f32,
        //         f32,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             *$Sample
        //         };
        //         (f64,
        //         f64,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             *$Sample
        //         };
        //         (f64,
        //         f32,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             *$Sample as f32
        //         };
        //         (f32,
        //         f64,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             *$Sample as f64
        //         };
        //         ($AsioTypeIdent:ident,
        //         f32,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             (*$Sample as f64 / ::std::$AsioTypeIdent::MAX as f64) as f32
        //         };
        //         ($AsioTypeIdent:ident,
        //         f64,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             *$Sample as f64 / ::std::$AsioTypeIdent::MAX as f64
        //         };
        //         (f32,
        //         $SampleType:ty,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             (*$Sample as f64 * ::std::$SampleTypeIdent::MAX as f64) as $SampleType
        //         };
        //         (f64,
        //         $SampleType:ty,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             (*$Sample as f64 * ::std::$SampleTypeIdent::MAX as f64) as $SampleType
        //         };
        //         ($AsioTypeIdent:ident,
        //         $SampleType:ty,
        //         $SampleTypeIdent:ident,
        //         $Sample:expr
        //         ) => {
        //             (*$Sample as i64 * ::std::$SampleTypeIdent::MAX as i64
        //                 / ::std::$AsioTypeIdent::MAX as i64) as $SampleType
        //         };
        //     };
        //     // This creates gets the buffer and interleaves it.
        //     // It allows it to be done based on the sample type.
        //     macro_rules! try_callback {
        //         ($SampleFormat:ident,
        //         $SampleType:ty,
        //         $SampleTypeIdent:ident,
        //         $AsioType:ty,
        //         $AsioTypeIdent:ident,
        //         $Buffers:expr,
        //         $BuffersType:ty,
        //         $BuffersTypeIdent:ident,
        //         $Endianness:expr,
        //         $ConvertEndian:expr
        //         ) => {
        //             // For each channel write the asio buffer to
        //             // the cpal buffer

        //             for (i, channel) in $Buffers.channel.iter_mut().enumerate() {
        //                 let buff_ptr = asio_stream.buffer_infos[i].buffers[index as usize]
        //                     as *mut $AsioType;
        //                 let asio_buffer: &'static [$AsioType] = std::slice::from_raw_parts(
        //                     buff_ptr,
        //                     asio_stream.buffer_size as usize,
        //                 );
        //                 for asio_s in asio_buffer.iter() {
        //                     channel.push($ConvertEndian(
        //                         convert_sample!(
        //                             $AsioTypeIdent,
        //                             $SampleType,
        //                             $SampleTypeIdent,
        //                             asio_s
        //                         ),
        //                         $Endianness,
        //                     ));
        //                 }
        //             }

        //             // interleave all the channels
        //             {
        //                 let $BuffersTypeIdent {
        //                     cpal: ref mut c_buffer,
        //                     channel: ref mut channels,
        //                 } = $Buffers;
        //                 au::interleave(&channels, c_buffer);
        //                 // Clear the per channel buffers
        //                 for c in channels.iter_mut() {
        //                     c.clear();
        //                 }
        //             }

        //             // Call the users callback with the buffer
        //             callback(
        //                 StreamId(count),
        //                 Ok(StreamData::Input {
        //                     buffer: UnknownTypeInputBuffer::$SampleFormat(::InputBuffer {
        //                         buffer: &$Buffers.cpal,
        //                     }),
        //                 }),
        //             );
        //         };
        //     };
        //     // Call the right buffer handler depending on types
        //     match stream_type {
        //         sys::AsioSampleType::ASIOSTInt32LSB => {
        //             try_callback!(
        //                 I16,
        //                 i16,
        //                 i16,
        //                 i32,
        //                 i32,
        //                 buffers.i16_buff,
        //                 I16Buffer,
        //                 I16Buffer,
        //                 Endian::Little,
        //                 convert_endian_from
        //             );
        //         }
        //         sys::AsioSampleType::ASIOSTInt16LSB => {
        //             try_callback!(
        //                 I16,
        //                 i16,
        //                 i16,
        //                 i16,
        //                 i16,
        //                 buffers.i16_buff,
        //                 I16Buffer,
        //                 I16Buffer,
        //                 Endian::Little,
        //                 convert_endian_from
        //             );
        //         }
        //         sys::AsioSampleType::ASIOSTInt32MSB => {
        //             try_callback!(
        //                 I16,
        //                 i16,
        //                 i16,
        //                 i32,
        //                 i32,
        //                 buffers.i16_buff,
        //                 I16Buffer,
        //                 I16Buffer,
        //                 Endian::Big,
        //                 convert_endian_from
        //             );
        //         }
        //         sys::AsioSampleType::ASIOSTInt16MSB => {
        //             try_callback!(
        //                 I16,
        //                 i16,
        //                 i16,
        //                 i16,
        //                 i16,
        //                 buffers.i16_buff,
        //                 I16Buffer,
        //                 I16Buffer,
        //                 Endian::Big,
        //                 convert_endian_from
        //             );
        //         }
        //         sys::AsioSampleType::ASIOSTFloat32LSB => {
        //             try_callback!(
        //                 F32,
        //                 f32,
        //                 f32,
        //                 f32,
        //                 f32,
        //                 buffers.f32_buff,
        //                 F32Buffer,
        //                 F32Buffer,
        //                 Endian::Little,
        //                 |a, _| a
        //             );
        //         }
        //         sys::AsioSampleType::ASIOSTFloat64LSB => {
        //             try_callback!(
        //                 F32,
        //                 f32,
        //                 f32,
        //                 f64,
        //                 f64,
        //                 buffers.f32_buff,
        //                 F32Buffer,
        //                 F32Buffer,
        //                 Endian::Little,
        //                 |a, _| a
        //             );
        //         }
        //         sys::AsioSampleType::ASIOSTFloat32MSB => {
        //             try_callback!(
        //                 F32,
        //                 f32,
        //                 f32,
        //                 f32,
        //                 f32,
        //                 buffers.f32_buff,
        //                 F32Buffer,
        //                 F32Buffer,
        //                 Endian::Big,
        //                 |a, _| a
        //             );
        //         }
        //         sys::AsioSampleType::ASIOSTFloat64MSB => {
        //             try_callback!(
        //                 F32,
        //                 f32,
        //                 f32,
        //                 f64,
        //                 f64,
        //                 buffers.f32_buff,
        //                 F32Buffer,
        //                 F32Buffer,
        //                 Endian::Big,
        //                 |a, _| a
        //             );
        //         }
        //         _ => println!("unsupported format {:?}", stream_type),
        //     }
        // });
        // // Create stream and set to paused
        // self.cpal_streams
        //     .lock()
        //     .unwrap()
        //     .push(Some(Stream { driver: driver.clone(), playing: false }));

        // Ok(StreamId(count))
    }

    /// Create the an output cpal stream.
    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError> {
        let Device { driver, .. } = device;
        let num_channels = format.channels.clone();
        let stream_type = driver.data_type().map_err(build_stream_err)?;
        let stream_buffer_size = self.get_output_stream(&driver, format, device)?;
        let channel_len = stream_buffer_size as usize;
        let cpal_num_samples = stream_buffer_size * num_channels as usize;
        let count = self.stream_count.fetch_add(1, Ordering::SeqCst);
        let asio_streams = self.asio_streams.clone();
        let cpal_streams = self.cpal_streams.clone();
        let callbacks = self.callbacks.clone();

        // Create buffers depending on data type.
        let stream_id = StreamId(count);
        let data_type = format.data_type;
        let len_bytes = cpal_num_samples * data_type.sample_size();
        let mut interleaved = vec![0u8; len_bytes];
        let mut silence_asio_buffer = SilenceAsioBuffer::default();

        sys::set_callback(move |buffer_index| unsafe {
            // If not playing, return early.
            if let Some(s) = cpal_streams.lock().unwrap().get(count) {
                if let Some(s) = s {
                    if !s.playing {
                        return ();
                    }
                }
            }

            // Acquire the stream and callback.
            let stream_lock = asio_streams.lock().unwrap();
            let ref asio_stream = match stream_lock.output {
                Some(ref asio_stream) => asio_stream,
                None => return (),
            };
            let mut callbacks = callbacks.lock().unwrap();
            let callback = match callbacks.as_mut() {
                Some(callback) => callback,
                None => return (),
            };

            // Silence the ASIO buffer that is about to be used.
            //
            // This checks if any other callbacks have already silenced the buffer associated with
            // the current `buffer_index`.
            //
            // If not, we will silence it and set the opposite buffer half to unsilenced.
            let silence = match buffer_index {
                0 if !silence_asio_buffer.first => {
                    silence_asio_buffer.first = true;
                    silence_asio_buffer.second = false;
                    true
                }
                0 => false,
                1 if !silence_asio_buffer.second => {
                    silence_asio_buffer.second = true;
                    silence_asio_buffer.first = false;
                    true
                }
                1 => false,
                _ => unreachable!("ASIO uses a double-buffer so there should only be 2"),
            };

            /// 1. Render the given callback to the given buffer of interleaved samples.
            /// 2. If required, silence the ASIO buffer.
            /// 3. Finally, write the interleaved data to the non-interleaved ASIO buffer,
            ///    performing endianness conversions as necessary.
            unsafe fn process_output_callback<A, B, F, G>(
                stream_id: StreamId,
                callback: &mut (dyn FnMut(StreamId, StreamDataResult) + Send),
                interleaved: &mut [u8],
                silence_asio_buffer: bool,
                asio_stream: &sys::AsioStream,
                buffer_index: usize,
                to_asio_sample: F,
                to_endianness: G,
            )
            where
                A: InterleavedSample,
                B: AsioSample,
                F: Fn(A) -> B,
                G: Fn(B) -> B,
            {
                // 1. Render interleaved buffer from callback.
                let interleaved: &mut [A] = cast_slice_mut(interleaved);
                callback(
                    stream_id,
                    Ok(StreamData::Output { buffer: A::unknown_type_output_buffer(interleaved) }),
                );

                // 2. Silence ASIO channels if necessary.
                let n_channels = interleaved.len() / asio_stream.buffer_size as usize;
                if silence_asio_buffer {
                    for ch_ix in 0..n_channels {
                        let asio_channel =
                            asio_channel_slice_mut::<B>(asio_stream, buffer_index, ch_ix);
                        asio_channel.iter_mut().for_each(|s| *s = to_endianness(B::SILENCE));
                    }
                }

                // 3. Write interleaved samples to ASIO channels, one channel at a time.
                for ch_ix in 0..n_channels {
                    let asio_channel =
                        asio_channel_slice_mut::<B>(asio_stream, buffer_index, ch_ix);
                    for (frame, s_asio) in interleaved.chunks(n_channels).zip(asio_channel) {
                        *s_asio = *s_asio + to_endianness(to_asio_sample(frame[ch_ix]));
                    }
                }
            }

            match (data_type, &stream_type) {
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt16LSB) => {
                    process_output_callback::<i16, i16, _, _>(
                        stream_id,
                        callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        buffer_index as usize,
                        std::convert::identity::<i16>,
                        to_le,
                    );
                }
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt16MSB) => {
                    process_output_callback::<i16, i16, _, _>(
                        stream_id,
                        callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        buffer_index as usize,
                        std::convert::identity::<i16>,
                        to_be,
                    );
                }

                // TODO: Handle endianness conversion for floats? We currently use the `PrimInt`
                // trait for the `to_le` and `to_be` methods, but this does not support floats.
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat32LSB) |
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat32MSB) => {
                    process_output_callback::<f32, f32, _, _>(
                        stream_id,
                        callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        buffer_index as usize,
                        std::convert::identity::<f32>,
                        std::convert::identity::<f32>,
                    );
                }

                // TODO: Add support for the following sample formats to CPAL and simplify the
                // `process_output_callback` function above by removing the unnecessary sample
                // conversion function.
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt32LSB) => {
                    process_output_callback::<i16, i32, _, _>(
                        stream_id,
                        callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        buffer_index as usize,
                        |s| (s as i32) << 16,
                        to_le,
                    );
                }
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt32MSB) => {
                    process_output_callback::<i16, i32, _, _>(
                        stream_id,
                        callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        buffer_index as usize,
                        |s| (s as i32) << 16,
                        to_be,
                    );
                }
                // TODO: Handle endianness conversion for floats? We currently use the `PrimInt`
                // trait for the `to_le` and `to_be` methods, but this does not support floats.
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat64LSB) |
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat64MSB) => {
                    process_output_callback::<f32, f64, _, _>(
                        stream_id,
                        callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        buffer_index as usize,
                        |s| s as f64,
                        std::convert::identity::<f64>,
                    );
                }

                unsupported_format_pair => {
                    unreachable!("`build_output_stream` should have returned with unsupported \
                                 format {:?}", unsupported_format_pair)
                }
            }
        });

        // Create the stream paused
        self.cpal_streams
            .lock()
            .unwrap()
            .push(Some(Stream { driver: driver.clone(), playing: false }));

        // Give the ID based on the stream count
        Ok(StreamId(count))
    }

    /// Play the cpal stream for the given ID.
    pub fn play_stream(&self, stream_id: StreamId) -> Result<(), PlayStreamError> {
        let mut streams = self.cpal_streams.lock().unwrap();
        if let Some(s) = streams.get_mut(stream_id.0).expect("Bad play stream index") {
            s.playing = true;
            // Calling play when already playing is a no-op
            s.driver.start().map_err(play_stream_err)?;
        }
        Ok(())
    }

    /// Pause the cpal stream for the given ID.
    ///
    /// Pause the ASIO streams if there are no other CPAL streams playing, as ASIO only allows
    /// stopping the entire driver.
    pub fn pause_stream(&self, stream_id: StreamId) -> Result<(), PauseStreamError> {
        let mut streams = self.cpal_streams.lock().unwrap();
        let streams_playing = streams.iter()
            .filter(|s| s.as_ref().map(|s| s.playing).unwrap_or(false))
            .count();
        if let Some(s) = streams.get_mut(stream_id.0).expect("Bad pause stream index") {
            if streams_playing <= 1 {
                s.driver.stop().map_err(pause_stream_err)?;
            }
            s.playing = false;
        }
        Ok(())
    }

    /// Destroy the cpal stream based on the ID.
    pub fn destroy_stream(&self, stream_id: StreamId) {
        // TODO: Should we not also remove an ASIO stream here?
        let mut streams = self.cpal_streams.lock().unwrap();
        streams.get_mut(stream_id.0).take();
    }

    /// Run the cpal callbacks
    pub fn run<F>(&self, mut callback: F) -> !
    where
        F: FnMut(StreamId, StreamDataResult) + Send,
    {
        let callback: &mut (FnMut(StreamId, StreamDataResult) + Send) = &mut callback;
        // Transmute needed to convince the compiler that the callback has a static lifetime
        *self.callbacks.lock().unwrap() = Some(unsafe { mem::transmute(callback) });
        loop {
            // A sleep here to prevent the loop being
            // removed in --release
            thread::sleep(Duration::new(1u64, 0u32));
        }
    }
}

/// Clean up if event loop is dropped.
/// Currently event loop is never dropped.
impl Drop for EventLoop {
    fn drop(&mut self) {
        *self.asio_streams.lock().unwrap() = sys::AsioStreams {
            output: None,
            input: None,
        };
    }
}

impl Silence for i16 {
    const SILENCE: Self = 0;
}

impl Silence for i32 {
    const SILENCE: Self = 0;
}

impl Silence for f32 {
    const SILENCE: Self = 0.0;
}

impl Silence for f64 {
    const SILENCE: Self = 0.0;
}

impl InterleavedSample for i16 {
    fn unknown_type_output_buffer(buffer: &mut [Self]) -> UnknownTypeOutputBuffer {
        UnknownTypeOutputBuffer::I16(::OutputBuffer { buffer })
    }
}

impl InterleavedSample for f32 {
    fn unknown_type_output_buffer(buffer: &mut [Self]) -> UnknownTypeOutputBuffer {
        UnknownTypeOutputBuffer::F32(::OutputBuffer { buffer })
    }
}

impl AsioSample for i16 {}

impl AsioSample for i32 {}

impl AsioSample for f32 {}

impl AsioSample for f64 {}

/// Cast a byte slice into a (immutable) slice of desired type.
///
/// Safety: it's up to the caller to ensure that the input slice has valid bit representations.
unsafe fn cast_slice<T>(v: &[u8]) -> &[T] {
    debug_assert!(v.len() % std::mem::size_of::<T>() == 0);
    std::slice::from_raw_parts(v.as_ptr() as *const T, v.len() / std::mem::size_of::<T>())
}

/// Cast a byte slice into a mutable slice of desired type.
///
/// Safety: it's up to the caller to ensure that the input slice has valid bit representations.
unsafe fn cast_slice_mut<T>(v: &mut [u8]) -> &mut [T] {
    debug_assert!(v.len() % std::mem::size_of::<T>() == 0);
    std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut T, v.len() / std::mem::size_of::<T>())
}

/// Helper function to convert to system endianness
fn to_le<T: PrimInt>(t: T) -> T {
    t.to_le()
}

/// Helper function to convert from system endianness
fn to_be<T: PrimInt>(t: T) -> T {
    t.to_be()
}

/// Shorthand for retrieving the asio buffer slice associated with a channel.
///
/// Safety: it's up to the user to ensure the slice is not used beyond the lifetime of
/// the stream and that this function is not called multiple times for the same
/// channel.
unsafe fn asio_channel_slice_mut<T>(
    asio_stream: &sys::AsioStream,
    buffer_index: usize,
    channel_index: usize,
) -> &mut [T] {
    let buff_ptr: *mut T = asio_stream
        .buffer_infos[channel_index]
        .buffers[buffer_index as usize]
        as *mut _;
    std::slice::from_raw_parts_mut(buff_ptr, asio_stream.buffer_size as usize)
}

/// Helper function to convert from system endianness
fn convert_endian_from<T: PrimInt>(sample: T, endian: Endian) -> T {
    match endian {
        Endian::Big => T::from_be(sample),
        Endian::Little => T::from_le(sample),
    }
}

fn build_stream_err(e: sys::AsioError) -> BuildStreamError {
    match e {
        sys::AsioError::NoDrivers |
        sys::AsioError::HardwareMalfunction => BuildStreamError::DeviceNotAvailable,
        sys::AsioError::InvalidInput |
        sys::AsioError::BadMode => BuildStreamError::InvalidArgument,
        err => {
            let description = format!("{}", err);
            BackendSpecificError { description }.into()
        }
    }
}

fn pause_stream_err(e: sys::AsioError) -> PauseStreamError {
    match e {
        sys::AsioError::NoDrivers |
        sys::AsioError::HardwareMalfunction => PauseStreamError::DeviceNotAvailable,
        err => {
            let description = format!("{}", err);
            BackendSpecificError { description }.into()
        }
    }
}

fn play_stream_err(e: sys::AsioError) -> PlayStreamError {
    match e {
        sys::AsioError::NoDrivers |
        sys::AsioError::HardwareMalfunction => PlayStreamError::DeviceNotAvailable,
        err => {
            let description = format!("{}", err);
            BackendSpecificError { description }.into()
        }
    }
}
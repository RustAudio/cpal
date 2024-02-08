extern crate asio_sys as sys;
extern crate num_traits;

use super::parking_lot::Mutex;
use super::Device;
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, InputCallbackInfo,
    OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat, SizedSample, StreamConfig,
    StreamError,
};
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct Stream {
    playing: Arc<AtomicBool>,
    // Ensure the `Driver` does not terminate until the last stream is dropped.
    driver: Arc<sys::Driver>,
    #[allow(dead_code)]
    asio_streams: Arc<Mutex<sys::AsioStreams>>,
    callback_id: sys::CallbackId,
}

impl Stream {
    pub fn play(&self) -> Result<(), PlayStreamError> {
        self.playing.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn pause(&self) -> Result<(), PauseStreamError> {
        self.playing.store(false, Ordering::SeqCst);
        Ok(())
    }
}

impl Device {
    pub fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_type = self.driver.input_data_type().map_err(build_stream_err)?;

        // Ensure that the desired sample type is supported.
        let expected_sample_format = super::device::convert_data_type(&stream_type)
            .ok_or(BuildStreamError::StreamConfigNotSupported)?;
        if sample_format != expected_sample_format {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        let num_channels = config.channels;
        let buffer_size = self.get_or_create_input_stream(config, sample_format)?;
        let cpal_num_samples = buffer_size * num_channels as usize;

        // Create the buffer depending on the size of the data type.
        let len_bytes = cpal_num_samples * sample_format.sample_size();
        let mut interleaved = vec![0u8; len_bytes];

        let stream_playing = Arc::new(AtomicBool::new(false));
        let playing = Arc::clone(&stream_playing);
        let asio_streams = self.asio_streams.clone();

        // Set the input callback.
        // This is most performance critical part of the ASIO bindings.
        let config = config.clone();
        let callback_id = self.driver.add_callback(move |callback_info| unsafe {
            // If not playing return early.
            if !playing.load(Ordering::SeqCst) {
                return;
            }

            // There is 0% chance of lock contention the host only locks when recreating streams.
            let stream_lock = asio_streams.lock();
            let asio_stream = match stream_lock.input {
                Some(ref asio_stream) => asio_stream,
                None => return,
            };

            /// 1. Write from the ASIO buffer to the interleaved CPAL buffer.
            /// 2. Deliver the CPAL buffer to the user callback.
            unsafe fn process_input_callback<A, D, F>(
                data_callback: &mut D,
                interleaved: &mut [u8],
                asio_stream: &sys::AsioStream,
                asio_info: &sys::CallbackInfo,
                sample_rate: crate::SampleRate,
                read_sample: F,
            ) where
                A: SizedSample,
                D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
                F: Fn(&[u8]) -> A,
            {
                // 1. Write the ASIO channels to the CPAL buffer.
                let interleaved: &mut [A] = cast_slice_mut(interleaved);
                let n_frames = asio_stream.buffer_size as usize;
                let n_channels = interleaved.len() / n_frames;
                let buffer_index = asio_info.buffer_index as usize;
                for ch_ix in 0..n_channels {
                    let asio_channel =
                        asio_channel_slice(asio_stream, buffer_index, ch_ix, size_of::<A>());

                    for (sample, s_asio) in interleaved
                        .iter_mut()
                        .skip(ch_ix)
                        .step_by(n_channels)
                        .zip(asio_channel.chunks_exact(size_of::<A>()))
                    {
                        *sample = read_sample(s_asio);
                    }
                }

                // 2. Deliver the interleaved buffer to the callback.
                let data = interleaved.as_mut_ptr() as *mut ();
                let len = interleaved.len();
                let data = Data::from_parts(data, len, A::FORMAT);
                let callback = system_time_to_stream_instant(asio_info.system_time);
                let delay = frames_to_duration(n_frames, sample_rate);
                let capture = callback
                    .sub(delay)
                    .expect("`capture` occurs before origin of alsa `StreamInstant`");
                let timestamp = crate::InputStreamTimestamp { callback, capture };
                let info = InputCallbackInfo { timestamp };
                data_callback(&data, &info);
            }

            match (&stream_type, sample_format) {
                (&sys::AsioSampleType::ASIOSTInt16LSB, SampleFormat::I16) => {
                    process_input_callback::<i16, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        LittleEndian::read_i16,
                    );
                }
                (&sys::AsioSampleType::ASIOSTInt16MSB, SampleFormat::I16) => {
                    process_input_callback::<i16, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        BigEndian::read_i16,
                    );
                }

                (&sys::AsioSampleType::ASIOSTFloat32LSB, SampleFormat::F32) => {
                    process_input_callback::<f32, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        LittleEndian::read_f32,
                    );
                }
                (&sys::AsioSampleType::ASIOSTFloat32MSB, SampleFormat::F32) => {
                    process_input_callback::<f32, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        BigEndian::read_f32,
                    );
                }

                (&sys::AsioSampleType::ASIOSTInt32LSB, SampleFormat::I32) => {
                    process_input_callback::<i32, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        LittleEndian::read_i32,
                    );
                }
                (&sys::AsioSampleType::ASIOSTInt32MSB, SampleFormat::I32) => {
                    process_input_callback::<i32, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        BigEndian::read_i32,
                    );
                }

                (&sys::AsioSampleType::ASIOSTFloat64LSB, SampleFormat::F64) => {
                    process_input_callback::<f64, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        LittleEndian::read_f64,
                    );
                }
                (&sys::AsioSampleType::ASIOSTFloat64MSB, SampleFormat::F64) => {
                    process_input_callback::<f64, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        BigEndian::read_f64,
                    );
                }

                unsupported_format_pair => unreachable!(
                    "`build_input_stream_raw` should have returned with unsupported \
                     format {:?}",
                    unsupported_format_pair
                ),
            }
        });

        let driver = self.driver.clone();
        let asio_streams = self.asio_streams.clone();

        // Immediately start the device?
        self.driver.start().map_err(build_stream_err)?;

        Ok(Stream {
            playing: stream_playing,
            driver,
            asio_streams,
            callback_id,
        })
    }

    pub fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_type = self.driver.output_data_type().map_err(build_stream_err)?;

        // Ensure that the desired sample type is supported.
        let expected_sample_format = super::device::convert_data_type(&stream_type)
            .ok_or(BuildStreamError::StreamConfigNotSupported)?;
        if sample_format != expected_sample_format {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        let num_channels = config.channels;
        let buffer_size = self.get_or_create_output_stream(config, sample_format)?;
        let cpal_num_samples = buffer_size * num_channels as usize;

        // Create buffers depending on data type.
        let len_bytes = cpal_num_samples * sample_format.sample_size();
        let mut interleaved = vec![0u8; len_bytes];
        let current_buffer_index = Arc::new(AtomicI32::new(-1));

        let stream_playing = Arc::new(AtomicBool::new(false));
        let playing = Arc::clone(&stream_playing);
        let asio_streams = self.asio_streams.clone();

        let config = config.clone();
        let callback_id = self.driver.add_callback(move |callback_info| unsafe {
            // If not playing, return early.
            if !playing.load(Ordering::SeqCst) {
                return;
            }

            // There is 0% chance of lock contention the host only locks when recreating streams.
            let mut stream_lock = asio_streams.lock();
            let asio_stream = match stream_lock.output {
                Some(ref mut asio_stream) => asio_stream,
                None => return,
            };

            // Silence the ASIO buffer that is about to be used.
            //
            // This checks if any other callbacks have already silenced the buffer associated with
            // the current `buffer_index`.
            let silence =
                current_buffer_index.load(Ordering::Acquire) != callback_info.buffer_index;

            if silence {
                current_buffer_index.store(callback_info.buffer_index, Ordering::Release);
            }

            /// 1. Render the given callback to the given buffer of interleaved samples.
            /// 2. If required, silence the ASIO buffer.
            /// 3. Finally, write the interleaved data to the non-interleaved ASIO buffer,
            ///    performing endianness conversions as necessary.
            unsafe fn process_output_callback<A, D, F>(
                data_callback: &mut D,
                interleaved: &mut [u8],
                silence_asio_buffer: bool,
                asio_stream: &mut sys::AsioStream,
                asio_info: &sys::CallbackInfo,
                sample_rate: crate::SampleRate,
                write_sample: F,
            ) where
                A: SizedSample,
                D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
                F: Fn(&mut [u8], A),
            {
                // 1. Render interleaved buffer from callback.
                let interleaved: &mut [A] = cast_slice_mut(interleaved);
                let data = interleaved.as_mut_ptr() as *mut ();
                let len = interleaved.len();
                let mut data = Data::from_parts(data, len, A::FORMAT);
                let callback = system_time_to_stream_instant(asio_info.system_time);
                let n_frames = asio_stream.buffer_size as usize;
                let delay = frames_to_duration(n_frames, sample_rate);
                let playback = callback
                    .add(delay)
                    .expect("`playback` occurs beyond representation supported by `StreamInstant`");
                let timestamp = crate::OutputStreamTimestamp { callback, playback };
                let info = OutputCallbackInfo { timestamp };
                data_callback(&mut data, &info);

                // 2. Silence ASIO channels if necessary.
                let n_channels = interleaved.len() / n_frames;
                let buffer_index = asio_info.buffer_index as usize;
                if silence_asio_buffer {
                    for ch_ix in 0..n_channels {
                        let asio_channel = asio_channel_slice_mut(
                            asio_stream,
                            buffer_index,
                            ch_ix,
                            size_of::<A>(),
                        );

                        asio_channel.fill(0);
                    }
                }

                // 3. Write interleaved samples to ASIO channels, one channel at a time.
                for ch_ix in 0..n_channels {
                    let asio_channel =
                        asio_channel_slice_mut(asio_stream, buffer_index, ch_ix, size_of::<A>());

                    for (sample, s_asio) in interleaved
                        .iter()
                        .skip(ch_ix)
                        .step_by(n_channels)
                        .zip(asio_channel.chunks_exact_mut(size_of::<A>()))
                    {
                        write_sample(s_asio, *sample);
                    }
                }
            }

            match (sample_format, &stream_type) {
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt16LSB) => {
                    process_output_callback(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        |buffer, sample: i16| {
                            LittleEndian::write_i16(
                                buffer,
                                sample.saturating_add(LittleEndian::read_i16(buffer)),
                            )
                        },
                    );
                }
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt16MSB) => {
                    process_output_callback(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        |buffer, sample: i16| {
                            BigEndian::write_i16(
                                buffer,
                                sample.saturating_add(BigEndian::read_i16(buffer)),
                            )
                        },
                    );
                }
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat32LSB) => {
                    process_output_callback(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        |buffer, sample: f32| {
                            LittleEndian::write_f32(buffer, sample + LittleEndian::read_f32(buffer))
                        },
                    );
                }

                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat32MSB) => {
                    process_output_callback(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        |buffer, sample: f32| {
                            BigEndian::write_f32(buffer, sample + BigEndian::read_f32(buffer))
                        },
                    );
                }

                (SampleFormat::I32, &sys::AsioSampleType::ASIOSTInt32LSB) => {
                    process_output_callback::<i32, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        |buffer, sample: i32| {
                            LittleEndian::write_i32(
                                buffer,
                                sample.saturating_add(LittleEndian::read_i32(buffer)),
                            )
                        },
                    );
                }
                (SampleFormat::I32, &sys::AsioSampleType::ASIOSTInt32MSB) => {
                    process_output_callback::<i32, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        |buffer, sample: i32| {
                            BigEndian::write_i32(
                                buffer,
                                sample.saturating_add(BigEndian::read_i32(buffer)),
                            )
                        },
                    );
                }

                (SampleFormat::F64, &sys::AsioSampleType::ASIOSTFloat64LSB) => {
                    process_output_callback::<f64, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        |buffer, sample: f64| {
                            LittleEndian::write_f64(buffer, sample + LittleEndian::read_f64(buffer))
                        },
                    );
                }

                (SampleFormat::F64, &sys::AsioSampleType::ASIOSTFloat64MSB) => {
                    process_output_callback::<f64, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        asio_stream,
                        callback_info,
                        config.sample_rate,
                        |buffer, sample: f64| {
                            BigEndian::write_f64(buffer, sample + BigEndian::read_f64(buffer))
                        },
                    );
                }

                unsupported_format_pair => unreachable!(
                    "`build_output_stream_raw` should have returned with unsupported \
                     format {:?}",
                    unsupported_format_pair
                ),
            }
        });

        let driver = self.driver.clone();
        let asio_streams = self.asio_streams.clone();

        // Immediately start the device?
        self.driver.start().map_err(build_stream_err)?;

        Ok(Stream {
            playing: stream_playing,
            driver,
            asio_streams,
            callback_id,
        })
    }

    /// Create a new CPAL Input Stream.
    ///
    /// If there is no existing ASIO Input Stream it will be created.
    ///
    /// On success, the buffer size of the stream is returned.
    fn get_or_create_input_stream(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
    ) -> Result<usize, BuildStreamError> {
        match self.default_input_config() {
            Ok(f) => {
                let num_asio_channels = f.channels;
                check_config(&self.driver, config, sample_format, num_asio_channels)
            }
            Err(_) => Err(BuildStreamError::StreamConfigNotSupported),
        }?;
        let num_channels = config.channels as usize;
        let mut streams = self.asio_streams.lock();

        let buffer_size = match config.buffer_size {
            BufferSize::Fixed(v) => Some(v as i32),
            BufferSize::Default => None,
        };

        // Either create a stream if thers none or had back the
        // size of the current one.
        match streams.input {
            Some(ref input) => Ok(input.buffer_size as usize),
            None => {
                let output = streams.output.take();
                self.driver
                    .prepare_input_stream(output, num_channels, buffer_size)
                    .map(|new_streams| {
                        let bs = match new_streams.input {
                            Some(ref inp) => inp.buffer_size as usize,
                            None => unreachable!(),
                        };
                        *streams = new_streams;
                        bs
                    })
                    .map_err(|ref e| {
                        println!("Error preparing stream: {}", e);
                        BuildStreamError::DeviceNotAvailable
                    })
            }
        }
    }

    /// Create a new CPAL Output Stream.
    ///
    /// If there is no existing ASIO Output Stream it will be created.
    fn get_or_create_output_stream(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
    ) -> Result<usize, BuildStreamError> {
        match self.default_output_config() {
            Ok(f) => {
                let num_asio_channels = f.channels;
                check_config(&self.driver, config, sample_format, num_asio_channels)
            }
            Err(_) => Err(BuildStreamError::StreamConfigNotSupported),
        }?;
        let num_channels = config.channels as usize;
        let mut streams = self.asio_streams.lock();

        let buffer_size = match config.buffer_size {
            BufferSize::Fixed(v) => Some(v as i32),
            BufferSize::Default => None,
        };

        // Either create a stream if thers none or had back the
        // size of the current one.
        match streams.output {
            Some(ref output) => Ok(output.buffer_size as usize),
            None => {
                let input = streams.input.take();
                self.driver
                    .prepare_output_stream(input, num_channels, buffer_size)
                    .map(|new_streams| {
                        let bs = match new_streams.output {
                            Some(ref out) => out.buffer_size as usize,
                            None => unreachable!(),
                        };
                        *streams = new_streams;
                        bs
                    })
                    .map_err(|ref e| {
                        println!("Error preparing stream: {}", e);
                        BuildStreamError::DeviceNotAvailable
                    })
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.driver.remove_callback(self.callback_id);
    }
}

fn asio_ns_to_double(val: sys::bindings::asio_import::ASIOTimeStamp) -> f64 {
    let two_raised_to_32 = 4294967296.0;
    val.lo as f64 + val.hi as f64 * two_raised_to_32
}

/// Asio retrieves system time via `timeGetTime` which returns the time in milliseconds.
fn system_time_to_stream_instant(
    system_time: sys::bindings::asio_import::ASIOTimeStamp,
) -> crate::StreamInstant {
    let systime_ns = asio_ns_to_double(system_time);
    let secs = systime_ns as i64 / 1_000_000_000;
    let nanos = (systime_ns as i64 - secs * 1_000_000_000) as u32;
    crate::StreamInstant::new(secs, nanos)
}

/// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
fn frames_to_duration(frames: usize, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate.0 as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

/// Check whether or not the desired config is supported by the stream.
///
/// Checks sample rate, data type and then finally the number of channels.
fn check_config(
    driver: &sys::Driver,
    config: &StreamConfig,
    sample_format: SampleFormat,
    num_asio_channels: u16,
) -> Result<(), BuildStreamError> {
    let StreamConfig {
        channels,
        sample_rate,
        buffer_size: _,
    } = config;
    // Try and set the sample rate to what the user selected.
    let sample_rate = sample_rate.0.into();
    if sample_rate != driver.sample_rate().map_err(build_stream_err)? {
        if driver
            .can_sample_rate(sample_rate)
            .map_err(build_stream_err)?
        {
            driver
                .set_sample_rate(sample_rate)
                .map_err(build_stream_err)?;
        } else {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }
    }
    // unsigned formats are not supported by asio
    match sample_format {
        SampleFormat::I16 | SampleFormat::I32 | SampleFormat::F32 => (),
        _ => return Err(BuildStreamError::StreamConfigNotSupported),
    }
    if *channels > num_asio_channels {
        return Err(BuildStreamError::StreamConfigNotSupported);
    }
    Ok(())
}

/// Cast a byte slice into a mutable slice of desired type.
///
/// Safety: it's up to the caller to ensure that the input slice has valid bit representations.
unsafe fn cast_slice_mut<T>(v: &mut [u8]) -> &mut [T] {
    debug_assert!(v.len() % std::mem::size_of::<T>() == 0);
    std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut T, v.len() / std::mem::size_of::<T>())
}

/// Shorthand for retrieving the asio buffer slice associated with a channel.
unsafe fn asio_channel_slice(
    asio_stream: &sys::AsioStream,
    buffer_index: usize,
    channel_index: usize,
    sample_size: usize,
) -> &[u8] {
    let buff_ptr = asio_stream.buffer_infos[channel_index].buffers[buffer_index] as *const u8;
    std::slice::from_raw_parts(buff_ptr, sample_size * asio_stream.buffer_size as usize)
}

/// Shorthand for retrieving the asio buffer slice associated with a channel.
unsafe fn asio_channel_slice_mut(
    asio_stream: &mut sys::AsioStream,
    buffer_index: usize,
    channel_index: usize,
    sample_size: usize,
) -> &mut [u8] {
    let buff_ptr = asio_stream.buffer_infos[channel_index].buffers[buffer_index] as *mut u8;
    std::slice::from_raw_parts_mut(buff_ptr, sample_size * asio_stream.buffer_size as usize)
}

fn build_stream_err(e: sys::AsioError) -> BuildStreamError {
    match e {
        sys::AsioError::NoDrivers | sys::AsioError::HardwareMalfunction => {
            BuildStreamError::DeviceNotAvailable
        }
        sys::AsioError::InvalidInput | sys::AsioError::BadMode => BuildStreamError::InvalidArgument,
        err => {
            let description = format!("{}", err);
            BackendSpecificError { description }.into()
        }
    }
}

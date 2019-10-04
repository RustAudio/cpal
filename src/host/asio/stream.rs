extern crate asio_sys as sys;
extern crate num_traits;

use self::num_traits::PrimInt;
use super::Device;
use std;
use std::sync::atomic::{Ordering, AtomicBool};
use std::sync::Arc;
use BackendSpecificError;
use BuildStreamError;
use Format;
use PauseStreamError;
use PlayStreamError;
use SampleFormat;
use StreamData;
use UnknownTypeInputBuffer;
use UnknownTypeOutputBuffer;
use StreamError;

/// Sample types whose constant silent value is known.
trait Silence {
    const SILENCE: Self;
}

/// Constraints on the interleaved sample buffer format required by the CPAL API.
trait InterleavedSample: Clone + Copy + Silence {
    fn unknown_type_input_buffer(&[Self]) -> UnknownTypeInputBuffer;
    fn unknown_type_output_buffer(&mut [Self]) -> UnknownTypeOutputBuffer;
}

/// Constraints on the ASIO sample types.
trait AsioSample: Clone + Copy + Silence + std::ops::Add<Self, Output = Self> {}

// Used to keep track of whether or not the current current asio stream buffer requires
// being silencing before summing audio.
#[derive(Default)]
struct SilenceAsioBuffer {
    first: bool,
    second: bool,
}

pub struct Stream {
    playing: Arc<AtomicBool>,
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

// TODO: drop implementation

impl Device {
    pub fn build_input_stream<D, E>(
        &self,
        format: &Format,
        mut data_callback: D,
        _error_callback: E,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(StreamData) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static
    {
        let stream_type = self.driver.input_data_type().map_err(build_stream_err)?;

        // Ensure that the desired sample type is supported.
        let data_type = super::device::convert_data_type(&stream_type)
            .ok_or(BuildStreamError::FormatNotSupported)?;
        if format.data_type != data_type {
            return Err(BuildStreamError::FormatNotSupported);
        }

        let num_channels = format.channels.clone();
        let asio_stream = self.get_or_create_input_stream(format)?;
        let cpal_num_samples = asio_stream.buffer_size as usize * num_channels as usize;

        // Create the buffer depending on the size of the data type.
        let len_bytes = cpal_num_samples * data_type.sample_size();
        let mut interleaved = vec![0u8; len_bytes];

        let stream_playing = Arc::new(AtomicBool::new(false));
        let playing = Arc::clone(&stream_playing);

        // Set the input callback.
        // This is most performance critical part of the ASIO bindings.
        self.driver.set_callback(move |buffer_index| unsafe {
            // If not playing return early.
            if !playing.load(Ordering::SeqCst) {
                return
            }

            /// 1. Write from the ASIO buffer to the interleaved CPAL buffer.
            /// 2. Deliver the CPAL buffer to the user callback.
            unsafe fn process_input_callback<A, B, D, F, G>(
                callback: &mut D,
                interleaved: &mut [u8],
                asio_stream: &sys::AsioStream,
                buffer_index: usize,
                from_endianness: F,
                to_cpal_sample: G,
            )
            where
                A: AsioSample,
                B: InterleavedSample,
                D: FnMut(StreamData) + Send + 'static,
                F: Fn(A) -> A,
                G: Fn(A) -> B,
            {
                // 1. Write the ASIO channels to the CPAL buffer.
                let interleaved: &mut [B] = cast_slice_mut(interleaved);
                let n_channels = interleaved.len() / asio_stream.buffer_size as usize;
                for ch_ix in 0..n_channels {
                    let asio_channel = asio_channel_slice::<A>(asio_stream, buffer_index, ch_ix);
                    for (frame, s_asio) in interleaved.chunks_mut(n_channels).zip(asio_channel) {
                        frame[ch_ix] = to_cpal_sample(from_endianness(*s_asio));
                    }
                }

                // 2. Deliver the interleaved buffer to the callback.
                callback(
                    StreamData::Input { buffer: B::unknown_type_input_buffer(interleaved) },
                );
            }

            match (&stream_type, data_type) {
                (&sys::AsioSampleType::ASIOSTInt16LSB, SampleFormat::I16) => {
                    process_input_callback::<i16, i16, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        &asio_stream,
                        buffer_index as usize,
                        from_le,
                        std::convert::identity::<i16>,
                    );
                }
                (&sys::AsioSampleType::ASIOSTInt16MSB, SampleFormat::I16) => {
                    process_input_callback::<i16, i16, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        &asio_stream,
                        buffer_index as usize,
                        from_be,
                        std::convert::identity::<i16>,
                    );
                }

                // TODO: Handle endianness conversion for floats? We currently use the `PrimInt`
                // trait for the `to_le` and `to_be` methods, but this does not support floats.
                (&sys::AsioSampleType::ASIOSTFloat32LSB, SampleFormat::F32) |
                (&sys::AsioSampleType::ASIOSTFloat32MSB, SampleFormat::F32) => {
                    process_input_callback::<f32, f32, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        &asio_stream,
                        buffer_index as usize,
                        std::convert::identity::<f32>,
                        std::convert::identity::<f32>,
                    );
                }

                // TODO: Add support for the following sample formats to CPAL and simplify the
                // `process_output_callback` function above by removing the unnecessary sample
                // conversion function.
                (&sys::AsioSampleType::ASIOSTInt32LSB, SampleFormat::I16) => {
                    process_input_callback::<i32, i16, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        &asio_stream,
                        buffer_index as usize,
                        from_le,
                        |s| (s >> 16) as i16,
                    );
                }
                (&sys::AsioSampleType::ASIOSTInt32MSB, SampleFormat::I16) => {
                    process_input_callback::<i32, i16, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        &asio_stream,
                        buffer_index as usize,
                        from_be,
                        |s| (s >> 16) as i16,
                    );
                }
                // TODO: Handle endianness conversion for floats? We currently use the `PrimInt`
                // trait for the `to_le` and `to_be` methods, but this does not support floats.
                (&sys::AsioSampleType::ASIOSTFloat64LSB, SampleFormat::F32) |
                (&sys::AsioSampleType::ASIOSTFloat64MSB, SampleFormat::F32) => {
                    process_input_callback::<f64, f32, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        &asio_stream,
                        buffer_index as usize,
                        std::convert::identity::<f64>,
                        |s| s as f32,
                    );
                }

                unsupported_format_pair => {
                    unreachable!("`build_input_stream` should have returned with unsupported \
                                 format {:?}", unsupported_format_pair)
                }
            }
        });

        // Immediately start the device?
        self.driver.start().map_err(build_stream_err)?;

        Ok(Stream { playing: stream_playing })
    }

    pub fn build_output_stream<D, E>(
        &self,
        format: &Format,
        mut data_callback: D,
        _error_callback: E,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(StreamData) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_type = self.driver.output_data_type().map_err(build_stream_err)?;

        // Ensure that the desired sample type is supported.
        let data_type = super::device::convert_data_type(&stream_type)
            .ok_or(BuildStreamError::FormatNotSupported)?;
        if format.data_type != data_type {
            return Err(BuildStreamError::FormatNotSupported);
        }

        let num_channels = format.channels.clone();
        let asio_stream = self.get_or_create_output_stream(format)?;
        let cpal_num_samples = asio_stream.buffer_size as usize * num_channels as usize;

        // Create buffers depending on data type.
        let len_bytes = cpal_num_samples * data_type.sample_size();
        let mut interleaved = vec![0u8; len_bytes];
        let mut silence_asio_buffer = SilenceAsioBuffer::default();

        let stream_playing = Arc::new(AtomicBool::new(false));
        let playing = Arc::clone(&stream_playing);

        self.driver.set_callback(move |buffer_index| unsafe {
            // If not playing, return early.
            if !playing.load(Ordering::SeqCst) {
                return
            }

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
            unsafe fn process_output_callback<A, B, D, F, G>(
                callback: &mut D,
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
                D: FnMut(StreamData) + Send + 'static,
                F: Fn(A) -> B,
                G: Fn(B) -> B,
            {
                // 1. Render interleaved buffer from callback.
                let interleaved: &mut [A] = cast_slice_mut(interleaved);
                let buffer = A::unknown_type_output_buffer(interleaved);
                callback(StreamData::Output { buffer });

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
                    process_output_callback::<i16, i16, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        &asio_stream,
                        buffer_index as usize,
                        std::convert::identity::<i16>,
                        to_le,
                    );
                }
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt16MSB) => {
                    process_output_callback::<i16, i16, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        &asio_stream,
                        buffer_index as usize,
                        std::convert::identity::<i16>,
                        to_be,
                    );
                }

                // TODO: Handle endianness conversion for floats? We currently use the `PrimInt`
                // trait for the `to_le` and `to_be` methods, but this does not support floats.
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat32LSB) |
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat32MSB) => {
                    process_output_callback::<f32, f32, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        &asio_stream,
                        buffer_index as usize,
                        std::convert::identity::<f32>,
                        std::convert::identity::<f32>,
                    );
                }

                // TODO: Add support for the following sample formats to CPAL and simplify the
                // `process_output_callback` function above by removing the unnecessary sample
                // conversion function.
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt32LSB) => {
                    process_output_callback::<i16, i32, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        &asio_stream,
                        buffer_index as usize,
                        |s| (s as i32) << 16,
                        to_le,
                    );
                }
                (SampleFormat::I16, &sys::AsioSampleType::ASIOSTInt32MSB) => {
                    process_output_callback::<i16, i32, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        &asio_stream,
                        buffer_index as usize,
                        |s| (s as i32) << 16,
                        to_be,
                    );
                }
                // TODO: Handle endianness conversion for floats? We currently use the `PrimInt`
                // trait for the `to_le` and `to_be` methods, but this does not support floats.
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat64LSB) |
                (SampleFormat::F32, &sys::AsioSampleType::ASIOSTFloat64MSB) => {
                    process_output_callback::<f32, f64, _, _, _>(
                        &mut data_callback,
                        &mut interleaved,
                        silence,
                        &asio_stream,
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

        // Immediately start the device?
        self.driver.start().map_err(build_stream_err)?;

        Ok(Stream { playing: stream_playing })
    }

    /// Create a new CPAL Input Stream.
    ///
    /// If there is no existing ASIO Input Stream it will be created.
    ///
    /// On success, the buffer size of the stream is returned.
    fn get_or_create_input_stream(
        &self,
        format: &Format,
    ) -> Result<sys::AsioStream, BuildStreamError> {
        match self.default_input_format() {
            Ok(f) => {
                let num_asio_channels = f.channels;
                check_format(&self.driver, format, num_asio_channels)
            },
            Err(_) => Err(BuildStreamError::FormatNotSupported),
        }?;
        let num_channels = format.channels as usize;
        let ref mut streams = *self.asio_streams.lock().unwrap();
        match streams {
            Some(streams) => match streams.input.take() {
                Some(input) => Ok(input),
                None => {
                    println!("ASIO streams have been already created");
                    Err(BuildStreamError::DeviceNotAvailable)
                }
            },
            None => {
                match self.driver.prepare_input_stream(None, num_channels) {
                    Ok(mut new_streams) => {
                        let input = new_streams.input.take().expect("missing input stream");
                        *streams = Some(new_streams);
                        Ok(input)
                    }
                    Err(e) => {
                        println!("Error preparing stream: {}", e);
                        Err(BuildStreamError::DeviceNotAvailable)
                    }
                }
            }
        }
    }

    /// Create a new CPAL Output Stream.
    ///
    /// If there is no existing ASIO Output Stream it will be created.
    fn get_or_create_output_stream(
        &self,
        format: &Format,
    ) -> Result<sys::AsioStream, BuildStreamError> {
        match self.default_output_format() {
            Ok(f) => {
                let num_asio_channels = f.channels;
                check_format(&self.driver, format, num_asio_channels)
            },
            Err(_) => Err(BuildStreamError::FormatNotSupported),
        }?;
        let num_channels = format.channels as usize;
        let ref mut streams = *self.asio_streams.lock().unwrap();
        match streams {
            Some(streams) => match streams.output.take() {
                Some(output) => Ok(output),
                None => {
                    println!("ASIO streams have been already created");
                    Err(BuildStreamError::DeviceNotAvailable)
                }
            },
            None => {
                match self.driver.prepare_output_stream(None, num_channels) {
                    Ok(mut new_streams) => {
                        let output = new_streams.output.take().expect("missing output stream");
                        *streams = Some(new_streams);
                        Ok(output)
                    }
                    Err(e) => {
                        println!("Error preparing stream: {}", e);
                        Err(BuildStreamError::DeviceNotAvailable)
                    }
                }
            }
        }
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
    fn unknown_type_input_buffer(buffer: &[Self]) -> UnknownTypeInputBuffer {
        UnknownTypeInputBuffer::I16(::InputBuffer { buffer })
    }

    fn unknown_type_output_buffer(buffer: &mut [Self]) -> UnknownTypeOutputBuffer {
        UnknownTypeOutputBuffer::I16(::OutputBuffer { buffer })
    }
}

impl InterleavedSample for f32 {
    fn unknown_type_input_buffer(buffer: &[Self]) -> UnknownTypeInputBuffer {
        UnknownTypeInputBuffer::F32(::InputBuffer { buffer })
    }

    fn unknown_type_output_buffer(buffer: &mut [Self]) -> UnknownTypeOutputBuffer {
        UnknownTypeOutputBuffer::F32(::OutputBuffer { buffer })
    }
}

impl AsioSample for i16 {}

impl AsioSample for i32 {}

impl AsioSample for f32 {}

impl AsioSample for f64 {}

/// Check whether or not the desired format is supported by the stream.
///
/// Checks sample rate, data type and then finally the number of channels.
fn check_format(
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

/// Cast a byte slice into a mutable slice of desired type.
///
/// Safety: it's up to the caller to ensure that the input slice has valid bit representations.
unsafe fn cast_slice_mut<T>(v: &mut [u8]) -> &mut [T] {
    debug_assert!(v.len() % std::mem::size_of::<T>() == 0);
    std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut T, v.len() / std::mem::size_of::<T>())
}

/// Helper function to convert to little endianness.
fn to_le<T: PrimInt>(t: T) -> T {
    t.to_le()
}

/// Helper function to convert to big endianness.
fn to_be<T: PrimInt>(t: T) -> T {
    t.to_be()
}

/// Helper function to convert from little endianness.
fn from_le<T: PrimInt>(t: T) -> T {
    T::from_le(t)
}

/// Helper function to convert from little endianness.
fn from_be<T: PrimInt>(t: T) -> T {
    T::from_be(t)
}

/// Shorthand for retrieving the asio buffer slice associated with a channel.
///
/// Safety: it's up to the user to ensure that this function is not called multiple times for the
/// same channel.
unsafe fn asio_channel_slice<T>(
    asio_stream: &sys::AsioStream,
    buffer_index: usize,
    channel_index: usize,
) -> &[T] {
    asio_channel_slice_mut(asio_stream, buffer_index, channel_index)
}

/// Shorthand for retrieving the asio buffer slice associated with a channel.
///
/// Safety: it's up to the user to ensure that this function is not called multiple times for the
/// same channel.
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

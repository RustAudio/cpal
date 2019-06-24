//! # How to use cpal
//!
//! Here are some concepts cpal exposes:
//!
//! - A `Device` is an audio device that may have any number of input and output streams.
//! - A stream is an open audio channel. Input streams allow you to receive audio data, output
//!   streams allow you to play audio data. You must choose which `Device` runs your stream before
//!   you create one.
//! - An `EventLoop` is a collection of streams being run by one or more `Device`. Each stream must
//!   belong to an `EventLoop`, and all the streams that belong to an `EventLoop` are managed
//!   together.
//!
//! The first step is to create an `EventLoop`:
//!
//! ```
//! use cpal::EventLoop;
//! let event_loop = EventLoop::new();
//! ```
//!
//! Then choose a `Device`. The easiest way is to use the default input or output `Device` via the
//! `default_input_device()` or `default_output_device()` functions. Alternatively you can
//! enumerate all the available devices with the `devices()` function. Beware that the
//! `default_*_device()` functions return an `Option` in case no device is available for that
//! stream type on the system.
//!
//! ```no_run
//! let device = cpal::default_output_device().expect("no output device available");
//! ```
//!
//! Before we can create a stream, we must decide what the format of the audio samples is going to
//! be. You can query all the supported formats with the `supported_input_formats()` and
//! `supported_output_formats()` methods. These produce a list of `SupportedFormat` structs which
//! can later be turned into actual `Format` structs. If you don't want to query the list of
//! formats, you can also build your own `Format` manually, but doing so could lead to an error
//! when building the stream if the format is not supported by the device.
//!
//! > **Note**: the `supported_formats()` method could return an error for example if the device
//! > has been disconnected.
//!
//! ```no_run
//! # let device = cpal::default_output_device().unwrap();
//! let mut supported_formats_range = device.supported_output_formats()
//!     .expect("error while querying formats");
//! let format = supported_formats_range.next()
//!     .expect("no supported format?!")
//!     .with_max_sample_rate();
//! ```
//!
//! Now that we have everything for the stream, we can create it from our event loop:
//!
//! ```no_run
//! # let device = cpal::default_output_device().unwrap();
//! # let format = device.supported_output_formats().unwrap().next().unwrap().with_max_sample_rate();
//! # let event_loop = cpal::EventLoop::new();
//! let stream_id = event_loop.build_output_stream(&device, &format).unwrap();
//! ```
//!
//! The value returned by `build_output_stream()` is of type `StreamId` and is an identifier that
//! will allow you to control the stream.
//!
//! Now we must start the stream. This is done with the `play_stream()` method on the event loop.
//!
//! ```no_run
//! # let event_loop: cpal::EventLoop = return;
//! # let stream_id: cpal::StreamId = return;
//! event_loop.play_stream(stream_id).expect("failed to play_stream");
//! ```
//!
//! Now everything is ready! We call `run()` on the `event_loop` to begin processing.
//!
//! ```no_run
//! # let event_loop = cpal::EventLoop::new();
//! event_loop.run(move |_stream_id, _stream_result| {
//!     // react to stream events and read or write stream data here
//! });
//! ```
//!
//! > **Note**: Calling `run()` will block the thread forever, so it's usually best done in a
//! > separate thread.
//!
//! While `run()` is running, the audio device of the user will from time to time call the callback
//! that you passed to this function. The callback gets passed the stream ID and an instance of type
//! `StreamData` that represents the data that must be read from or written to. The inner
//! `UnknownTypeOutputBuffer` can be one of `I16`, `U16` or `F32` depending on the format that was
//! passed to `build_output_stream`.
//!
//! In this example, we simply fill the given output buffer with zeroes.
//!
//! ```no_run
//! use cpal::{StreamData, UnknownTypeOutputBuffer};
//!
//! # let event_loop = cpal::EventLoop::new();
//! event_loop.run(move |stream_id, stream_result| {
//!     let stream_data = match stream_result {
//!         Ok(data) => data,
//!         Err(err) => {
//!             eprintln!("an error occurred on stream {:?}: {}", stream_id, err);
//!             return;
//!         }
//!         _ => return,
//!     };
//!
//!     match stream_data {
//!         StreamData::Output { buffer: UnknownTypeOutputBuffer::U16(mut buffer) } => {
//!             for elem in buffer.iter_mut() {
//!                 *elem = u16::max_value() / 2;
//!             }
//!         },
//!         StreamData::Output { buffer: UnknownTypeOutputBuffer::I16(mut buffer) } => {
//!             for elem in buffer.iter_mut() {
//!                 *elem = 0;
//!             }
//!         },
//!         StreamData::Output { buffer: UnknownTypeOutputBuffer::F32(mut buffer) } => {
//!             for elem in buffer.iter_mut() {
//!                 *elem = 0.0;
//!             }
//!         },
//!         _ => (),
//!     }
//! });
//! ```

#![recursion_limit = "512"]

extern crate failure;
#[cfg(target_os = "windows")]
#[macro_use]
extern crate lazy_static;
// Extern crate declarations with `#[macro_use]` must unfortunately be at crate root.
#[cfg(target_os = "emscripten")]
#[macro_use]
extern crate stdweb;

pub use samples_formats::{Sample, SampleFormat};

#[cfg(not(any(windows, target_os = "linux", target_os = "freebsd",
              target_os = "macos", target_os = "ios", target_os = "emscripten")))]
use null as cpal_impl;

use failure::Fail;
use std::fmt;
use std::iter;
use std::ops::{Deref, DerefMut};

mod null;
mod samples_formats;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[path = "alsa/mod.rs"]
mod cpal_impl;

#[cfg(windows)]
#[path = "wasapi/mod.rs"]
mod cpal_impl;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[path = "coreaudio/mod.rs"]
mod cpal_impl;

#[cfg(target_os = "emscripten")]
#[path = "emscripten/mod.rs"]
mod cpal_impl;

/// An opaque type that identifies a device that is capable of either audio input or output.
///
/// Please note that `Device`s may become invalid if they get disconnected. Therefore all the
/// methods that involve a device return a `Result`.
#[derive(Clone, PartialEq, Eq)]
pub struct Device(cpal_impl::Device);

/// Collection of streams managed together.
///
/// Created with the [`new`](struct.EventLoop.html#method.new) method.
pub struct EventLoop(cpal_impl::EventLoop);

/// Identifier of a stream within the `EventLoop`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(cpal_impl::StreamId);

/// Number of channels.
pub type ChannelCount = u16;

/// The number of samples processed per second for a single channel of audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SampleRate(pub u32);

/// The format of an input or output audio stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub data_type: SampleFormat,
}

/// Describes a range of supported stream formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedFormat {
    pub channels: ChannelCount,
    /// Minimum value for the samples rate of the supported formats.
    pub min_sample_rate: SampleRate,
    /// Maximum value for the samples rate of the supported formats.
    pub max_sample_rate: SampleRate,
    /// Type of data expected by the device.
    pub data_type: SampleFormat,
}

/// Stream data passed to the `EventLoop::run` callback.
pub enum StreamData<'a> {
    Input {
        buffer: UnknownTypeInputBuffer<'a>,
    },
    Output {
        buffer: UnknownTypeOutputBuffer<'a>,
    },
}

/// Stream data passed to the `EventLoop::run` callback, or an error in the case that the device
/// was invalidated or some backend-specific error occurred.
pub type StreamDataResult<'a> = Result<StreamData<'a>, StreamError>;

/// Represents a buffer containing audio data that may be read.
///
/// This struct implements the `Deref` trait targeting `[T]`. Therefore this buffer can be read the
/// same way as reading from a `Vec` or any other kind of Rust array.
// TODO: explain audio stuff in general
// TODO: remove the wrapper and just use slices in next major version
pub struct InputBuffer<'a, T: 'a>
where
    T: Sample,
{
    buffer: &'a [T],
}

/// Represents a buffer that must be filled with audio data. The buffer in unfilled state may
/// contain garbage values.
///
/// This struct implements the `Deref` and `DerefMut` traits to `[T]`. Therefore writing to this
/// buffer is done in the same way as writing to a `Vec` or any other kind of Rust array.
// TODO: explain audio stuff in general
// TODO: remove the wrapper and just use slices
#[must_use]
pub struct OutputBuffer<'a, T: 'a>
where
    T: Sample,
{
    buffer: &'a mut [T],
}

/// This is the struct that is provided to you by cpal when you want to read samples from a buffer.
///
/// Since the type of data is only known at runtime, you have to read the right buffer.
pub enum UnknownTypeInputBuffer<'a> {
    /// Samples whose format is `u16`.
    U16(InputBuffer<'a, u16>),
    /// Samples whose format is `i16`.
    I16(InputBuffer<'a, i16>),
    /// Samples whose format is `f32`.
    F32(InputBuffer<'a, f32>),
}

/// This is the struct that is provided to you by cpal when you want to write samples to a buffer.
///
/// Since the type of data is only known at runtime, you have to fill the right buffer.
pub enum UnknownTypeOutputBuffer<'a> {
    /// Samples whose format is `u16`.
    U16(OutputBuffer<'a, u16>),
    /// Samples whose format is `i16`.
    I16(OutputBuffer<'a, i16>),
    /// Samples whose format is `f32`.
    F32(OutputBuffer<'a, f32>),
}

/// An iterator yielding all `Device`s currently available to the system.
///
/// See [`devices()`](fn.devices.html).
pub struct Devices(cpal_impl::Devices);

/// A `Devices` yielding only *input* devices.
pub type InputDevices = iter::Filter<Devices, fn(&Device) -> bool>;

/// A `Devices` yielding only *output* devices.
pub type OutputDevices = iter::Filter<Devices, fn(&Device) -> bool>;

/// An iterator that produces a list of input stream formats supported by the device.
///
/// See [`Device::supported_input_formats()`](struct.Device.html#method.supported_input_formats).
pub struct SupportedInputFormats(cpal_impl::SupportedInputFormats);

/// An iterator that produces a list of output stream formats supported by the device.
///
/// See [`Device::supported_output_formats()`](struct.Device.html#method.supported_output_formats).
pub struct SupportedOutputFormats(cpal_impl::SupportedOutputFormats);

/// Some error has occurred that is specific to the backend from which it was produced.
///
/// This error is often used as a catch-all in cases where:
///
/// - It is unclear exactly what error might be produced by the backend API.
/// - It does not make sense to add a variant to the enclosing error type.
/// - No error was expected to occur at all, but we return an error to avoid the possibility of a
///   `panic!` caused by some unforseen or unknown reason.
///
/// **Note:** If you notice a `BackendSpecificError` that you believe could be better handled in a
/// cross-platform manner, please create an issue or submit a pull request with a patch that adds
/// the necessary error variant to the appropriate error enum.
#[derive(Clone, Debug, Fail)]
#[fail(display = "A backend-specific error has occurred: {}", description)]
pub struct BackendSpecificError {
    pub description: String
}

/// An error that might occur while attempting to enumerate the available devices on a system.
#[derive(Debug, Fail)]
pub enum DevicesError {
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    }
}

/// An error that may occur while attempting to retrieve a device name.
#[derive(Debug, Fail)]
pub enum DeviceNameError {
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    }
}

/// Error that can happen when enumerating the list of supported formats.
#[derive(Debug, Fail)]
pub enum SupportedFormatsError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(display = "The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,
    /// We called something the C-Layer did not understand
    #[fail(display = "Invalid argument passed to the backend. For example, this happens when trying to read capture capabilities when the device does not support it.")]
    InvalidArgument,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    }
}

/// May occur when attempting to request the default input or output stream format from a `Device`.
#[derive(Debug, Fail)]
pub enum DefaultFormatError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(display = "The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,
    /// Returned if e.g. the default input format was requested on an output-only audio device.
    #[fail(display = "The requested stream type is not supported by the device.")]
    StreamTypeNotSupported,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    }
}

/// Error that can happen when creating a `Stream`.
#[derive(Debug, Fail)]
pub enum BuildStreamError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(display = "The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,
    /// The required format is not supported.
    #[fail(display = "The requested stream format is not supported by the device.")]
    FormatNotSupported,
    /// We called something the C-Layer did not understand
    ///
    /// On ALSA device functions called with a feature they do not support will yield this. E.g.
    /// Trying to use capture capabilities on an output only format yields this.
    #[fail(display = "The requested device does not support this capability (invalid argument)")]
    InvalidArgument,
    /// Occurs if adding a new Stream ID would cause an integer overflow.
    #[fail(display = "Adding a new stream ID would cause an overflow")]
    StreamIdOverflow,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    }
}

/// Errors that might occur when calling `play_stream`.
///
/// As of writing this, only macOS may immediately return an error while calling this method. This
/// is because both the alsa and wasapi backends only enqueue these commands and do not process
/// them immediately.
#[derive(Debug, Fail)]
pub enum PlayStreamError {
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    }
}

/// Errors that might occur when calling `pause_stream`.
///
/// As of writing this, only macOS may immediately return an error while calling this method. This
/// is because both the alsa and wasapi backends only enqueue these commands and do not process
/// them immediately.
#[derive(Debug, Fail)]
pub enum PauseStreamError {
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    }
}

/// Errors that might occur while a stream is running.
#[derive(Debug, Fail)]
pub enum StreamError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(display = "The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    }
}

/// An iterator yielding all `Device`s currently available to the system.
///
/// Can be empty if the system does not support audio in general.
#[inline]
pub fn devices() -> Result<Devices, DevicesError> {
    Ok(Devices(cpal_impl::Devices::new()?))
}

/// An iterator yielding all `Device`s currently available to the system that support one or more
/// input stream formats.
///
/// Can be empty if the system does not support audio input.
pub fn input_devices() -> Result<InputDevices, DevicesError> {
    fn supports_input(device: &Device) -> bool {
        device.supported_input_formats()
            .map(|mut iter| iter.next().is_some())
            .unwrap_or(false)
    }
    Ok(devices()?.filter(supports_input))
}

/// An iterator yielding all `Device`s currently available to the system that support one or more
/// output stream formats.
///
/// Can be empty if the system does not support audio output.
pub fn output_devices() -> Result<OutputDevices, DevicesError> {
    fn supports_output(device: &Device) -> bool {
        device.supported_output_formats()
            .map(|mut iter| iter.next().is_some())
            .unwrap_or(false)
    }
    Ok(devices()?.filter(supports_output))
}

/// The default input audio device on the system.
///
/// Returns `None` if no input device is available.
pub fn default_input_device() -> Option<Device> {
    cpal_impl::default_input_device().map(Device)
}

/// The default output audio device on the system.
///
/// Returns `None` if no output device is available.
pub fn default_output_device() -> Option<Device> {
    cpal_impl::default_output_device().map(Device)
}

impl Device {
    /// The human-readable name of the device.
    #[inline]
    pub fn name(&self) -> Result<String, DeviceNameError> {
        self.0.name()
    }

    /// An iterator yielding formats that are supported by the backend.
    ///
    /// Can return an error if the device is no longer valid (eg. it has been disconnected).
    #[inline]
    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, SupportedFormatsError> {
        Ok(SupportedInputFormats(self.0.supported_input_formats()?))
    }

    /// An iterator yielding output stream formats that are supported by the device.
    ///
    /// Can return an error if the device is no longer valid (eg. it has been disconnected).
    #[inline]
    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        Ok(SupportedOutputFormats(self.0.supported_output_formats()?))
    }

    /// The default input stream format for the device.
    #[inline]
    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        self.0.default_input_format()
    }

    /// The default output stream format for the device.
    #[inline]
    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        self.0.default_output_format()
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl EventLoop {
    /// Initializes a new events loop.
    #[inline]
    pub fn new() -> EventLoop {
        EventLoop(cpal_impl::EventLoop::new())
    }

    /// Creates a new input stream that will run from the given device and with the given format.
    ///
    /// On success, returns an identifier for the stream.
    ///
    /// Can return an error if the device is no longer valid, or if the input stream format is not
    /// supported by the device.
    #[inline]
    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError>
    {
        self.0.build_input_stream(&device.0, format).map(StreamId)
    }

    /// Creates a new output stream that will play on the given device and with the given format.
    ///
    /// On success, returns an identifier for the stream.
    ///
    /// Can return an error if the device is no longer valid, or if the output stream format is not
    /// supported by the device.
    #[inline]
    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError>
    {
        self.0.build_output_stream(&device.0, format).map(StreamId)
    }

    /// Instructs the audio device that it should start playing the stream with the given ID.
    ///
    /// Has no effect is the stream was already playing.
    ///
    /// Only call this after you have submitted some data, otherwise you may hear some glitches.
    ///
    /// # Panic
    ///
    /// If the stream does not exist, this function can either panic or be a no-op.
    ///
    #[inline]
    pub fn play_stream(&self, stream: StreamId) -> Result<(), PlayStreamError> {
        self.0.play_stream(stream.0)
    }

    /// Instructs the audio device that it should stop playing the stream with the given ID.
    ///
    /// Has no effect is the stream was already paused.
    ///
    /// If you call `play` afterwards, the playback will resume where it was.
    ///
    /// # Panic
    ///
    /// If the stream does not exist, this function can either panic or be a no-op.
    ///
    #[inline]
    pub fn pause_stream(&self, stream: StreamId) -> Result<(), PauseStreamError> {
        self.0.pause_stream(stream.0)
    }

    /// Destroys an existing stream.
    ///
    /// # Panic
    ///
    /// If the stream does not exist, this function can either panic or be a no-op.
    ///
    #[inline]
    pub fn destroy_stream(&self, stream_id: StreamId) {
        self.0.destroy_stream(stream_id.0)
    }

    /// Takes control of the current thread and begins the stream processing.
    ///
    /// > **Note**: Since it takes control of the thread, this method is best called on a separate
    /// > thread.
    ///
    /// Whenever a stream needs to be fed some data, the closure passed as parameter is called.
    /// You can call the other methods of `EventLoop` without getting a deadlock.
    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamDataResult) + Send
    {
        self.0.run(move |id, data| callback(StreamId(id), data))
    }
}

impl SupportedFormat {
    /// Turns this `SupportedFormat` into a `Format` corresponding to the maximum samples rate.
    #[inline]
    pub fn with_max_sample_rate(self) -> Format {
        Format {
            channels: self.channels,
            sample_rate: self.max_sample_rate,
            data_type: self.data_type,
        }
    }

    /// A comparison function which compares two `SupportedFormat`s in terms of their priority of
    /// use as a default stream format.
    ///
    /// Some backends do not provide a default stream format for their audio devices. In these
    /// cases, CPAL attempts to decide on a reasonable default format for the user. To do this we
    /// use the "greatest" of all supported stream formats when compared with this method.
    ///
    /// Formats are prioritised by the following heuristics:
    ///
    /// **Channels**:
    ///
    /// - Stereo
    /// - Mono
    /// - Max available channels
    ///
    /// **Sample format**:
    /// - f32
    /// - i16
    /// - u16
    ///
    /// **Sample rate**:
    ///
    /// - 44100 (cd quality)
    /// - Max sample rate
    pub fn cmp_default_heuristics(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::Equal;
        use SampleFormat::{F32, I16, U16};

        let cmp_stereo = (self.channels == 2).cmp(&(other.channels == 2));
        if cmp_stereo != Equal {
            return cmp_stereo;
        }

        let cmp_mono = (self.channels == 1).cmp(&(other.channels == 1));
        if cmp_mono != Equal {
            return cmp_mono;
        }

        let cmp_channels = self.channels.cmp(&other.channels);
        if cmp_channels != Equal {
            return cmp_channels;
        }

        let cmp_f32 = (self.data_type == F32).cmp(&(other.data_type == F32));
        if cmp_f32 != Equal {
            return cmp_f32;
        }

        let cmp_i16 = (self.data_type == I16).cmp(&(other.data_type == I16));
        if cmp_i16 != Equal {
            return cmp_i16;
        }

        let cmp_u16 = (self.data_type == U16).cmp(&(other.data_type == U16));
        if cmp_u16 != Equal {
            return cmp_u16;
        }

        const HZ_44100: SampleRate = SampleRate(44_100);
        let r44100_in_self = self.min_sample_rate <= HZ_44100
            && HZ_44100 <= self.max_sample_rate;
        let r44100_in_other = other.min_sample_rate <= HZ_44100
            && HZ_44100 <= other.max_sample_rate;
        let cmp_r44100 = r44100_in_self.cmp(&r44100_in_other);
        if cmp_r44100 != Equal {
            return cmp_r44100;
        }

        self.max_sample_rate.cmp(&other.max_sample_rate)
    }
}

impl<'a, T> Deref for InputBuffer<'a, T>
    where T: Sample
{
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        self.buffer
    }
}

impl<'a, T> Deref for OutputBuffer<'a, T>
    where T: Sample
{
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        self.buffer
    }
}

impl<'a, T> DerefMut for OutputBuffer<'a, T>
    where T: Sample
{
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        self.buffer
    }
}

impl<'a> UnknownTypeInputBuffer<'a> {
    /// Returns the length of the buffer in number of samples.
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            &UnknownTypeInputBuffer::U16(ref buf) => buf.len(),
            &UnknownTypeInputBuffer::I16(ref buf) => buf.len(),
            &UnknownTypeInputBuffer::F32(ref buf) => buf.len(),
        }
    }
}

impl<'a> UnknownTypeOutputBuffer<'a> {
    /// Returns the length of the buffer in number of samples.
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            &UnknownTypeOutputBuffer::U16(ref buf) => buf.len(),
            &UnknownTypeOutputBuffer::I16(ref buf) => buf.len(),
            &UnknownTypeOutputBuffer::F32(ref buf) => buf.len(),
        }
    }
}

impl From<Format> for SupportedFormat {
    #[inline]
    fn from(format: Format) -> SupportedFormat {
        SupportedFormat {
            channels: format.channels,
            min_sample_rate: format.sample_rate,
            max_sample_rate: format.sample_rate,
            data_type: format.data_type,
        }
    }
}

impl Iterator for Devices {
    type Item = Device;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(Device)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl Iterator for SupportedInputFormats {
    type Item = SupportedFormat;

    #[inline]
    fn next(&mut self) -> Option<SupportedFormat> {
        self.0.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl Iterator for SupportedOutputFormats {
    type Item = SupportedFormat;

    #[inline]
    fn next(&mut self) -> Option<SupportedFormat> {
        self.0.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl From<BackendSpecificError> for DevicesError {
    fn from(err: BackendSpecificError) -> Self {
        DevicesError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for DeviceNameError {
    fn from(err: BackendSpecificError) -> Self {
        DeviceNameError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for SupportedFormatsError {
    fn from(err: BackendSpecificError) -> Self {
        SupportedFormatsError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for DefaultFormatError {
    fn from(err: BackendSpecificError) -> Self {
        DefaultFormatError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for BuildStreamError {
    fn from(err: BackendSpecificError) -> Self {
        BuildStreamError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for PlayStreamError {
    fn from(err: BackendSpecificError) -> Self {
        PlayStreamError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for PauseStreamError {
    fn from(err: BackendSpecificError) -> Self {
        PauseStreamError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for StreamError {
    fn from(err: BackendSpecificError) -> Self {
        StreamError::BackendSpecific { err }
    }
}

// If a backend does not provide an API for retrieving supported formats, we query it with a bunch
// of commonly used rates. This is always the case for wasapi and is sometimes the case for alsa.
//
// If a rate you desire is missing from this list, feel free to add it!
#[cfg(target_os = "windows")]
const COMMON_SAMPLE_RATES: &'static [SampleRate] = &[
    SampleRate(5512),
    SampleRate(8000),
    SampleRate(11025),
    SampleRate(16000),
    SampleRate(22050),
    SampleRate(32000),
    SampleRate(44100),
    SampleRate(48000),
    SampleRate(64000),
    SampleRate(88200),
    SampleRate(96000),
    SampleRate(176400),
    SampleRate(192000),
];

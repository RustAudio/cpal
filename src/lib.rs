//! # How to use cpal
//!
//! Here are some concepts cpal exposes:
//!
//! - A [**Host**](./struct.Host.html) provides access to the available audio devices on the system.
//!   Some platforms have more than one host available, but every platform supported by CPAL has at
//!   least one [**DefaultHost**](./struct.Host.html) that is guaranteed to be available.
//! - A [**Device**](./struct.Device.html) is an audio device that may have any number of input and
//!   output streams.
//! - A [**Stream**](./trait.Stream.html) is an open flow of audio data. Input streams allow you to
//!   receive audio data, output streams allow you to play audio data. You must choose which
//!   **Device** will run your stream before you can create one. Often, a default device can be
//!   retrieved via the **Host**.
//!
//! The first step is to initialise the `Host`:
//!
//! ```
//! use cpal::traits::HostTrait;
//! let host = cpal::default_host();
//! ```
//!
//! Then choose an available `Device`. The easiest way is to use the default input or output
//! `Device` via the `default_input_device()` or `default_output_device()` functions. Alternatively
//! you can enumerate all the available devices with the `devices()` function. Beware that the
//! `default_*_device()` functions return an `Option` in case no device is available for that
//! stream type on the system.
//!
//! ```no_run
//! # use cpal::traits::HostTrait;
//! # let host = cpal::default_host();
//! let device = host.default_output_device().expect("no output device available");
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
//! use cpal::traits::{DeviceTrait, HostTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! let mut supported_formats_range = device.supported_output_formats()
//!     .expect("error while querying formats");
//! let format = supported_formats_range.next()
//!     .expect("no supported format?!")
//!     .with_max_sample_rate();
//! ```
//!
//! Now that we have everything for the stream, we are ready to create it from our selected device:
//!
//! ```no_run
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let format = device.default_output_format().unwrap();
//! let stream = device.build_output_stream(
//!     &format,
//!     move |data| {
//!         // react to stream events and read or write stream data here.
//!     },
//!     move |err| {
//!         // react to errors here.
//!     },
//! );
//! ```
//!
//! While the stream is running, the selected audio device will periodically call the data callback
//! that was passed to the function. The callback is passed an instance of type `StreamData` that
//! represents the data that must be read from or written to. The inner `UnknownTypeOutputBuffer`
//! can be one of `I16`, `U16` or `F32` depending on the format that was passed to
//! `build_output_stream`.
//!
//! > **Note**: Creating and running a stream will *not* block the thread. On modern platforms, the
//! > given callback is called by a dedicated, high-priority thread responsible for delivering
//! > audio data to the system's audio device in a timely manner. On older platforms that only
//! > provide a blocking API (e.g. ALSA), CPAL will create a thread in order to consistently
//! > provide non-blocking behaviour. *If this is an issue for your platform or design, please
//! > share your issue and use-case with the CPAL team on the github issue tracker for
//! > consideration.*
//!
//! In this example, we simply fill the given output buffer with zeroes.
//!
//! ```no_run
//! use cpal::{StreamData, UnknownTypeOutputBuffer};
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let format = device.default_output_format().unwrap();
//! let stream = device.build_output_stream(
//!     &format,
//!     move |data| {
//!         match data {
//!             StreamData::Output { buffer: UnknownTypeOutputBuffer::U16(mut buffer) } => {
//!                 for elem in buffer.iter_mut() {
//!                     *elem = u16::max_value() / 2;
//!                 }
//!             },
//!             StreamData::Output { buffer: UnknownTypeOutputBuffer::I16(mut buffer) } => {
//!                 for elem in buffer.iter_mut() {
//!                     *elem = 0;
//!                 }
//!             },
//!             StreamData::Output { buffer: UnknownTypeOutputBuffer::F32(mut buffer) } => {
//!                 for elem in buffer.iter_mut() {
//!                     *elem = 0.0;
//!                 }
//!             },
//!             _ => (),
//!         }
//!     },
//!     move |err| {
//!         eprintln!("an error occurred on the output audio stream: {}", err);
//!     },
//! );
//! ```
//!
//! Not all platforms automatically run the stream upon creation. To ensure the stream has started,
//! we can use `Stream::play`.
//!
//! ```no_run
//! # use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let format = device.default_output_format().unwrap();
//! # let stream = device.build_output_stream(&format, move |_data| {}, move |_err| {}).unwrap();
//! stream.play().unwrap();
//! ```
//!
//! Some devices support pausing the audio stream. This can be useful for saving energy in moments
//! of silence.
//!
//! ```no_run
//! # use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let format = device.default_output_format().unwrap();
//! # let stream = device.build_output_stream(&format, move |_data| {}, move |_err| {}).unwrap();
//! stream.pause().unwrap();

#![recursion_limit = "512"]

#[cfg(target_os = "windows")]
#[macro_use]
extern crate lazy_static;
// Extern crate declarations with `#[macro_use]` must unfortunately be at crate root.
#[cfg(target_os = "emscripten")]
#[macro_use]
extern crate stdweb;
extern crate thiserror;

pub use error::*;
pub use platform::{
    ALL_HOSTS, available_hosts, default_host, Device, Devices, Host, host_from_id,
    HostId, Stream, SupportedInputFormats, SupportedOutputFormats,
};
pub use samples_formats::{Sample, SampleFormat};
use std::ops::{Deref, DerefMut};

mod error;
mod host;
pub mod platform;
mod samples_formats;
pub mod traits;

/// A host's device iterator yielding only *input* devices.
pub type InputDevices<I> = std::iter::Filter<I, fn(&<I as Iterator>::Item) -> bool>;

/// A host's device iterator yielding only *output* devices.
pub type OutputDevices<I> = std::iter::Filter<I, fn(&<I as Iterator>::Item) -> bool>;

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

//! # How to use cpal
//!
//! Here are some concepts cpal exposes:
//!
//! - An endpoint is a target where the data of the audio channel will be played.
//! - A voice is an open audio channel which you can stream audio data to. You have to choose which
//!   endpoint your voice targets before you create one.
//! - An event loop is a collection of voices. Each voice must belong to an event loop, and all the
//!   voices that belong to an event loop are managed together.
//!
//! In order to play a sound, you first need to create an event loop:
//!
//! ```
//! use cpal::EventLoop;
//! let event_loop = EventLoop::new();
//! ```
//!
//! Then choose an endpoint. You can either use the default endpoint with the `default_endpoint()`
//! function, or enumerate all the available endpoints with the `endpoints()` function. Beware that
//! `default_endpoint()` returns an `Option` in case no endpoint is available on the system.
//!
//! ```
//! // Note: we call `unwrap()` because it is convenient, but you should avoid doing that in a real
//! // code.
//! let endpoint = cpal::default_endpoint().expect("no endpoint is available");
//! ```
//!
//! Before we can create a voice, we must decide what the format of the audio samples is going to
//! be. You can query all the supported formats with the `supported_formats()` method, which
//! produces a list of `SupportedFormat` structs which can later be turned into actual `Format`
//! structs. If you don't want to query the list of formats, you can also build your own `Format`
//! manually, but doing so could lead to an error when building the voice if the format ends up not
//! being supported.
//!
//! > **Note**: the `supported_formats()` method could return an error for example if the device
//! > has been disconnected.
//!
//! ```no_run
//! # let endpoint = cpal::default_endpoint().unwrap();
//! let mut supported_formats_range = endpoint.supported_formats()
//!                                           .expect("error while querying formats");
//! let format = supported_formats_range.next().expect("no supported format?!")
//!                                     .with_max_samples_rate();
//! ```
//!
//! Now that we have everything, we can create a voice from that event loop:
//!
//! ```no_run
//! # let endpoint = cpal::default_endpoint().unwrap();
//! # let format = endpoint.supported_formats().unwrap().next().unwrap().with_max_samples_rate();
//! # let event_loop = cpal::EventLoop::new();
//! let voice_id = event_loop.build_voice(&endpoint, &format).unwrap();
//! ```
//!
//! The value returned by `build_voice()` is of type `VoiceId` and is an identifier that will
//! allow you to control the voice.
//!
//! There is a last step to perform before going forward, which is to start the voice. This is done
//! with the `play()` method on the event loop.
//!
//! ```
//! # let event_loop: cpal::EventLoop = return;
//! # let voice_id: cpal::VoiceId = return;
//! event_loop.play(voice_id);
//! ```
//!
//! Once everything is done, you must call `run()` on the `event_loop`.
//!
//! ```no_run
//! # let event_loop = cpal::EventLoop::new();
//! event_loop.run(move |_voice_id, _buffer| {
//!     // write data to `buffer` here
//! });
//! ```
//!
//! > **Note**: Calling `run()` will block the thread forever, so it's usually best done in a
//! > separate thread.
//!
//! While `run()` is running, the audio device of the user will from time to time call the callback
//! that you passed to this function. The callback gets passed the voice ID, and a struct of type
//! `UnknownTypeBuffer` that represents the buffer that must be filled with audio samples. The
//! `UnknownTypeBuffer` can be one of `I16`, `U16` or `F32` depending on the format that was passed
//! to `build_voice`.
//!
//! In this example, we simply simply fill the buffer with zeroes.
//!
//! ```no_run
//! use cpal::UnknownTypeBuffer;
//!
//! # let event_loop = cpal::EventLoop::new();
//! event_loop.run(move |_voice_id, mut buffer| {
//!     match buffer {
//!         UnknownTypeBuffer::U16(mut buffer) => {
//!             for elem in buffer.iter_mut() {
//!                 *elem = u16::max_value() / 2;
//!             }
//!         },
//!         UnknownTypeBuffer::I16(mut buffer) => {
//!             for elem in buffer.iter_mut() {
//!                 *elem = 0;
//!             }
//!         },
//!         UnknownTypeBuffer::F32(mut buffer) => {
//!             for elem in buffer.iter_mut() {
//!                 *elem = 0.0;
//!             }
//!         },
//!     }
//! });
//! ```

#![recursion_limit = "512"]

#[macro_use]
extern crate lazy_static;

// Extern crate declarations with `#[macro_use]` must unfortunately be at crate root.
#[cfg(target_os = "emscripten")]
#[macro_use]
extern crate stdweb;

pub use samples_formats::{Sample, SampleFormat};

#[cfg(all(not(windows), not(target_os = "linux"), not(target_os = "freebsd"),
            not(target_os = "macos"), not(target_os = "ios"), not(target_os = "emscripten")))]
use null as cpal_impl;

use std::error::Error;
use std::fmt;
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

/// An iterator for the list of formats that are supported by the backend.
pub struct EndpointsIterator(cpal_impl::EndpointsIterator);

impl Iterator for EndpointsIterator {
    type Item = Endpoint;

    #[inline]
    fn next(&mut self) -> Option<Endpoint> {
        self.0.next().map(Endpoint)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

/// Return an iterator to the list of formats that are supported by the system.
#[inline]
pub fn endpoints() -> EndpointsIterator {
    EndpointsIterator(Default::default())
}

/// Deprecated. Use `endpoints()` instead.
#[inline]
#[deprecated]
pub fn get_endpoints_list() -> EndpointsIterator {
    EndpointsIterator(Default::default())
}

/// Return the default endpoint, or `None` if no device is available.
#[inline]
pub fn default_endpoint() -> Option<Endpoint> {
    cpal_impl::default_endpoint().map(Endpoint)
}

/// Deprecated. Use `default_endpoint()` instead.
#[inline]
#[deprecated]
pub fn get_default_endpoint() -> Option<Endpoint> {
    default_endpoint()
}

/// An opaque type that identifies an end point.
#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint(cpal_impl::Endpoint);

impl Endpoint {
    /// Returns an iterator that produces the list of formats that are supported by the backend.
    #[inline]
    pub fn supported_formats(&self) -> Result<SupportedFormatsIterator, FormatsEnumerationError> {
        Ok(SupportedFormatsIterator(self.0.supported_formats()?))
    }

    /// Deprecated. Use `supported_formats` instead.
    #[inline]
    #[deprecated]
    pub fn get_supported_formats_list(
        &self)
        -> Result<SupportedFormatsIterator, FormatsEnumerationError> {
        self.supported_formats()
    }

    /// Returns the name of the endpoint.
    #[inline]
    pub fn name(&self) -> String {
        self.0.name()
    }

    /// Deprecated. Use `name()` instead.
    #[deprecated]
    #[inline]
    pub fn get_name(&self) -> String {
        self.name()
    }
}

/// Number of channels.
pub type ChannelsCount = u16;

/// Possible position of a channel.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ChannelPosition {
    FrontLeft,
    FrontRight,
    FrontCenter,
    LowFrequency,
    BackLeft,
    BackRight,
    FrontLeftOfCenter,
    FrontRightOfCenter,
    BackCenter,
    SideLeft,
    SideRight,
    TopCenter,
    TopFrontLeft,
    TopFrontCenter,
    TopFrontRight,
    TopBackLeft,
    TopBackCenter,
    TopBackRight,
}

///
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SamplesRate(pub u32);

/// Describes a format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub channels: Vec<ChannelPosition>,
    pub samples_rate: SamplesRate,
    pub data_type: SampleFormat,
}

/// An iterator that produces a list of formats supported by the endpoint.
pub struct SupportedFormatsIterator(cpal_impl::SupportedFormatsIterator);

impl Iterator for SupportedFormatsIterator {
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

/// Describes a format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedFormat {
    pub channels: Vec<ChannelPosition>,
    pub min_samples_rate: SamplesRate,
    pub max_samples_rate: SamplesRate,
    pub data_type: SampleFormat,
}

impl SupportedFormat {
    /// Builds a corresponding `Format` corresponding to the maximum samples rate.
    #[inline]
    pub fn with_max_samples_rate(self) -> Format {
        Format {
            channels: self.channels,
            samples_rate: self.max_samples_rate,
            data_type: self.data_type,
        }
    }
}

impl From<Format> for SupportedFormat {
    #[inline]
    fn from(format: Format) -> SupportedFormat {
        SupportedFormat {
            channels: format.channels,
            min_samples_rate: format.samples_rate,
            max_samples_rate: format.samples_rate,
            data_type: format.data_type,
        }
    }
}

pub struct EventLoop(cpal_impl::EventLoop);

impl EventLoop {
    /// Initializes a new events loop.
    #[inline]
    pub fn new() -> EventLoop {
        EventLoop(cpal_impl::EventLoop::new())
    }

    /// Creates a new voice that will play on the given endpoint and with the given format.
    ///
    /// On success, returns an identifier for the voice.
    #[inline]
    pub fn build_voice(&self, endpoint: &Endpoint, format: &Format)
                       -> Result<VoiceId, CreationError> {
        self.0.build_voice(&endpoint.0, format).map(VoiceId)
    }

    /// Destroys an existing voice.
    ///
    /// # Panic
    ///
    /// If the voice doesn't exist, this function can either panic or be a no-op.
    ///
    #[inline]
    pub fn destroy_voice(&self, voice_id: VoiceId) {
        self.0.destroy_voice(voice_id.0)
    }

    /// Takes control of the current thread and processes the sounds.
    ///
    /// Whenever a voice needs to be fed some data, the closure passed as parameter is called.
    /// **Note**: Calling other methods of the events loop from the callback will most likely
    /// deadlock. Don't do that. Maybe this will change in the future.
    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(VoiceId, UnknownTypeBuffer)
    {
        self.0.run(move |id, buf| callback(VoiceId(id), buf))
    }

    /// Sends a command to the audio device that it should start playing.
    ///
    /// Has no effect is the voice was already playing.
    ///
    /// Only call this after you have submitted some data, otherwise you may hear
    /// some glitches.
    ///
    /// # Panic
    ///
    /// If the voice doesn't exist, this function can either panic or be a no-op.
    ///
    #[inline]
    pub fn play(&self, voice: VoiceId) {
        self.0.play(voice.0)
    }

    /// Sends a command to the audio device that it should stop playing.
    ///
    /// Has no effect is the voice was already paused.
    ///
    /// If you call `play` afterwards, the playback will resume exactly where it was.
    ///
    /// # Panic
    ///
    /// If the voice doesn't exist, this function can either panic or be a no-op.
    ///
    #[inline]
    pub fn pause(&self, voice: VoiceId) {
        self.0.pause(voice.0)
    }
}

/// Identifier of a voice in an events loop.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoiceId(cpal_impl::VoiceId);

/// This is the struct that is provided to you by cpal when you want to write samples to a buffer.
///
/// Since the type of data is only known at runtime, you have to fill the right buffer.
pub enum UnknownTypeBuffer<'a> {
    /// Samples whose format is `u16`.
    U16(Buffer<'a, u16>),
    /// Samples whose format is `i16`.
    I16(Buffer<'a, i16>),
    /// Samples whose format is `f32`.
    F32(Buffer<'a, f32>),
}

impl<'a> UnknownTypeBuffer<'a> {
    /// Returns the length of the buffer in number of samples.
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            &UnknownTypeBuffer::U16(ref buf) => buf.target.as_ref().unwrap().len(),
            &UnknownTypeBuffer::I16(ref buf) => buf.target.as_ref().unwrap().len(),
            &UnknownTypeBuffer::F32(ref buf) => buf.target.as_ref().unwrap().len(),
        }
    }
}

/// Error that can happen when enumerating the list of supported formats.
#[derive(Debug)]
pub enum FormatsEnumerationError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    DeviceNotAvailable,
}

impl fmt::Display for FormatsEnumerationError {
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{}", self.description())
    }
}

impl Error for FormatsEnumerationError {
    #[inline]
    fn description(&self) -> &str {
        match self {
            &FormatsEnumerationError::DeviceNotAvailable => {
                "The requested device is no longer available (for example, it has been unplugged)."
            },
        }
    }
}

/// Error that can happen when creating a `Voice`.
#[derive(Debug)]
pub enum CreationError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    DeviceNotAvailable,

    /// The required format is not supported.
    FormatNotSupported,
}

impl fmt::Display for CreationError {
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{}", self.description())
    }
}

impl Error for CreationError {
    #[inline]
    fn description(&self) -> &str {
        match self {
            &CreationError::DeviceNotAvailable => {
                "The requested device is no longer available (for example, it has been unplugged)."
            },

            &CreationError::FormatNotSupported => {
                "The requested samples format is not supported by the device."
            },
        }
    }
}

/// Represents a buffer that must be filled with audio data.
///
/// You should destroy this object as soon as possible. Data is only committed when it
/// is destroyed.
#[must_use]
pub struct Buffer<'a, T: 'a>
    where T: Sample
{
    // Always contains something, taken by `Drop`
    // TODO: change that
    target: Option<cpal_impl::Buffer<'a, T>>,
}

impl<'a, T> Deref for Buffer<'a, T>
    where T: Sample
{
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        panic!("It is forbidden to read from the audio buffer");
    }
}

impl<'a, T> DerefMut for Buffer<'a, T>
    where T: Sample
{
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        self.target.as_mut().unwrap().buffer()
    }
}

impl<'a, T> Drop for Buffer<'a, T>
    where T: Sample
{
    #[inline]
    fn drop(&mut self) {
        self.target.take().unwrap().finish();
    }
}

/*!
# How to use cpal

In order to play a sound, first you need to create an `EventLoop` and a `Voice`.

```no_run
// getting the default sound output of the system (can return `None` if nothing is supported)
let endpoint = cpal::get_default_endpoint().unwrap();

// note that the user can at any moment disconnect the device, therefore all operations return
// a `Result` to handle this situation

// getting a format for the PCM
let format = endpoint.get_supported_formats_list().unwrap().next().unwrap();

let event_loop = cpal::EventLoop::new();

let (voice, mut samples_stream) = cpal::Voice::new(&endpoint, &format, &event_loop).unwrap();
```

The `voice` can be used to control the play/pause of the output, while the `samples_stream` can
be used to register a callback that will be called whenever the backend is ready to get data.
See the documentation of `futures-rs` for more info about how to use streams.

```no_run
# extern crate futures;
# extern crate cpal;
# use std::sync::Arc;
use futures::stream::Stream;
use futures::task;
# struct MyExecutor;
# impl task::Executor for MyExecutor {
#     fn execute(&self, r: task::Run) {
#         r.run();
#     }
# }
# fn main() {
# let mut samples_stream: cpal::SamplesStream = unsafe { std::mem::uninitialized() };
# let my_executor = Arc::new(MyExecutor);

task::spawn(samples_stream.for_each(move |buffer| -> Result<_, ()> {
    // write data to `buffer` here

    Ok(())
})).execute(my_executor);
# }
```

TODO: add example

After you have registered a callback, call `play`:

```no_run
# let mut voice: cpal::Voice = unsafe { std::mem::uninitialized() };
voice.play();
```

And finally, run the event loop:

```no_run
# let mut event_loop: cpal::EventLoop = unsafe { std::mem::uninitialized() };
event_loop.run();
```

Calling `run()` will block the thread forever, so it's usually best done in a separate thread.

While `run()` is running, the audio device of the user will call the callbacks you registered
from time to time.

*/

#[macro_use]
extern crate lazy_static;
extern crate libc;

pub use samples_formats::{Sample, SampleFormat};

#[cfg(all(not(windows), not(target_os = "linux"), not(target_os = "freebsd"),
            not(target_os = "macos"), not(target_os = "ios")))]
use null as cpal_impl;

use std::error::Error;
use std::fmt;
use std::ops::{Deref, DerefMut};

//mod null;     // TODO: restore
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
    type Item = Format;

    #[inline]
    fn next(&mut self) -> Option<Format> {
        self.0.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
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
                       -> Result<VoiceId, CreationError>
    {
        self.0.build_voice(&endpoint.0, format).map(VoiceId)
    }

    /// Destroys an existing voice.
    ///
    /// # Panic
    ///
    /// Panics if the voice doesn't exist.
    ///
    #[inline]
    pub fn destroy_voice(&self, voice_id: VoiceId) {
        self.0.destroy_voice(voice_id.0)
    }

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
    #[inline]
    pub fn play(&self, voice: VoiceId) {
        self.0.play(voice.0)
    }

    /// Sends a command to the audio device that it should stop playing.
    ///
    /// Has no effect is the voice was already paused.
    ///
    /// If you call `play` afterwards, the playback will resume exactly where it was.
    #[inline]
    pub fn pause(&self, voice: VoiceId) {
        self.0.pause(voice.0)
    }
}

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
    // also contains something, taken by `Drop`

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

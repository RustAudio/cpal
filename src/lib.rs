//! # How to use cpal
//!
//! Here are some concepts cpal exposes:
//!
//! - A [`Host`] provides access to the available audio devices on the system.
//!   Some platforms have more than one host available, but every platform supported by CPAL has at
//!   least one [default_host] that is guaranteed to be available.
//! - A [`Device`] is an audio device that may have any number of input and
//!   output streams.
//! - A [`Stream`] is an open flow of audio data. Input streams allow you to
//!   receive audio data, output streams allow you to play audio data. You must choose which
//!   [Device] will run your stream before you can create one. Often, a default device can be
//!   retrieved via the [Host].
//!
//! ## Quick Start
//!
//! The easiest way to create an audio stream is using the builder API:
//!
//! ```no_run
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//!
//! let host = cpal::default_host();
//! let device = host.default_output_device().expect("no output device available");
//!
//! // Create stream with device defaults
//! let stream = device.default_output_config()?
//!     .build_output_stream(
//!         |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
//!             // Fill buffer with audio data
//!             for sample in data.iter_mut() {
//!                 *sample = 0.0; // Fill with silence
//!             }
//!         },
//!         |err| eprintln!("Stream error: {}", err),
//!         None, // None=blocking, Some(Duration)=timeout
//!     )?;
//!
//! stream.play()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Manual Configuration (Advanced)
//!
//! For more control, you can manually configure the stream parameters:
//!
//! ```no_run
//! use cpal::traits::{DeviceTrait, HostTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! let mut supported_configs_range = device.supported_output_configs()
//!     .expect("error while querying configs");
//! let supported_config = supported_configs_range.next()
//!     .expect("no supported config?!")
//!     .with_max_sample_rate();
//! ```
//!
//! The first step is to initialise the [`Host`]:
//!
//! ```
//! use cpal::traits::HostTrait;
//! let host = cpal::default_host();
//! ```
//!
//! Then choose an available [`Device`]. The easiest way is to use the default input or output
//! `Device` via the [`default_input_device()`] or [`default_output_device()`] methods on `host`.
//!
//! Alternatively, you can enumerate all the available devices with the [`devices()`] method.
//! Beware that the `default_*_device()` functions return an `Option<Device>` in case no device
//! is available for that stream type on the system.
//!
//! ```no_run
//! # use cpal::traits::HostTrait;
//! # let host = cpal::default_host();
//! let device = host.default_output_device().expect("no output device available");
//! ```
//!
//! Before we can create a stream manually, we must decide what the configuration of the audio
//! stream is going to be. You can query all the supported configurations with the
//! [`supported_input_configs()`] and [`supported_output_configs()`] methods.
//! These produce a list of [`SupportedStreamConfigRange`] structs which can later be turned into
//! actual [`SupportedStreamConfig`] structs.
//!
//! If you don't want to query the list of configs,
//! you can also build your own [`StreamConfig`] manually, but doing so could lead to an error when
//! building the stream if the config is not supported by the device.
//!
//! > **Note**: the `supported_input/output_configs()` methods
//! > could return an error for example if the device has been disconnected.
//!
//! For manual stream creation with explicit sample format handling:
//!
//! ```no_run
//! use cpal::{Data, Sample, SampleFormat, FromSample};
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let supported_config = device.default_output_config().unwrap();
//! let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);
//! let sample_format = supported_config.sample_format();
//! let config = supported_config.into();
//! let stream = match sample_format {
//!     SampleFormat::F32 => device.build_output_stream(&config, write_silence::<f32>, err_fn, None),
//!     SampleFormat::I16 => device.build_output_stream(&config, write_silence::<i16>, err_fn, None),
//!     SampleFormat::U16 => device.build_output_stream(&config, write_silence::<u16>, err_fn, None),
//!     sample_format => panic!("Unsupported sample format '{sample_format}'")
//! }.unwrap();
//!
//! fn write_silence<T: Sample>(data: &mut [T], _: &cpal::OutputCallbackInfo) {
//!     for sample in data.iter_mut() {
//!         *sample = Sample::EQUILIBRIUM;
//!     }
//! }
//! ```
//!
//! While the stream is running, the selected audio device will periodically call the data callback
//! that was passed to the function. The callback is passed an instance of either [`&Data` or
//! `&mut Data`](Data) depending on whether the stream is an input stream or output stream
//! respectively.
//!
//! For most use cases, the callback closures can simply capture variables by value. However, if
//! you need to capture variables that implement `FnOnce` traits or transfer ownership of complex
//! state, you may need to use `move` in your closure.
//!
//! > **Note**: Creating and running a stream will *not* block the thread. On modern platforms, the
//! > given callback is called by a dedicated, high-priority thread responsible for delivering
//! > audio data to the system's audio device in a timely manner. On older platforms that only
//! > provide a blocking API (e.g. ALSA), CPAL will create a thread in order to consistently
//! > provide non-blocking behaviour (currently this is a thread per stream, but this may change to
//! > use a single thread for all streams). *If this is an issue for your platform or design,
//! > please share your issue and use-case with the CPAL team on the GitHub issue tracker for
//! > consideration.*
//!
//! Not all platforms automatically run the stream upon creation. To ensure the stream has started,
//! we must use [`Stream::play`](traits::StreamTrait::play).
//!
//! ```no_run
//! # use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let supported_config = device.default_output_config().unwrap();
//! # let sample_format = supported_config.sample_format();
//! # let config = supported_config.into();
//! # let data_fn = move |_data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {};
//! # let err_fn = move |_err| {};
//! # let stream = device.build_output_stream_raw(&config, sample_format, data_fn, err_fn, None).unwrap();
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
//! # let supported_config = device.default_output_config().unwrap();
//! # let sample_format = supported_config.sample_format();
//! # let config = supported_config.into();
//! # let data_fn = move |_data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {};
//! # let err_fn = move |_err| {};
//! # let stream = device.build_output_stream_raw(&config, sample_format, data_fn, err_fn, None).unwrap();
//! stream.pause().unwrap();
//! ```
//!
//! ## Cross-Platform Stream Configuration with Platform-Specific Optimizations
//!
//! The builder API provides true cross-platform compatibility while allowing platform-specific
//! optimizations. The same code compiles and runs on all platforms without conditional compilation:
//!
//! ```no_run
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! use cpal::BufferSize;
//!
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! // This EXACT code works on ALL platforms - no #[cfg(...)] needed!
//! let stream = device.default_output_config()?
//!     .with_buffer_size(BufferSize::Fixed(512))
//!     .on_alsa(|alsa| alsa.periods(2))                    // ALSA optimization
//!     .on_jack(|jack| jack.client_name("MyApp".into()))   // JACK integration
//!     .build_output_stream(
//!         move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
//!             // Generate audio - move is often needed for complex state
//!             for sample in data.iter_mut() {
//!                 *sample = 0.0; // Fill with silence
//!             }
//!         },
//!         move |err| eprintln!("Stream error: {}", err),
//!         None, // None=blocking, Some(Duration)=timeout
//!     )?;
//!
//! stream.play()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! You can also build configurations from scratch with explicit parameters:
//!
//! ```no_run
//! use cpal::{BufferSize, SampleFormat, SampleRate, StreamConfigBuilder};
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//!
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! let stream = StreamConfigBuilder::new()
//!     .channels(2)
//!     .sample_rate(SampleRate(48_000))
//!     .sample_format(SampleFormat::F32)
//!     .buffer_size(BufferSize::Fixed(1024))
//!     .on_alsa(|alsa| alsa.periods(4))  // More periods for stability
//!     .on_jack(|jack| {
//!         jack.client_name("CustomApp".into())
//!             .connect_ports_automatically(true)
//!     })
//!     .build_output_stream(
//!         &device,
//!         move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
//!             // Audio processing logic here
//!             for sample in data.iter_mut() {
//!                 *sample = 0.0;
//!             }
//!         },
//!         move |err| eprintln!("Stream error: {}", err),
//!         None
//!     )?;
//!
//! stream.play()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [`default_input_device()`]: traits::HostTrait::default_input_device
//! [`default_output_device()`]: traits::HostTrait::default_output_device
//! [`devices()`]: traits::HostTrait::devices
//! [`supported_input_configs()`]: traits::DeviceTrait::supported_input_configs
//! [`supported_output_configs()`]: traits::DeviceTrait::supported_output_configs

#![recursion_limit = "2048"]

// Extern crate declarations with `#[macro_use]` must unfortunately be at crate root.
#[cfg(target_os = "emscripten")]
#[macro_use]
extern crate wasm_bindgen;
#[cfg(target_os = "emscripten")]
extern crate js_sys;
#[cfg(target_os = "emscripten")]
extern crate web_sys;

pub use error::*;
pub use platform::{
    available_hosts, default_host, host_from_id, Device, Devices, Host, HostId, Stream,
    SupportedInputConfigs, SupportedOutputConfigs, ALL_HOSTS,
};
pub use samples_formats::{FromSample, Sample, SampleFormat, SizedSample, I24, I48, U24, U48};
use std::convert::TryInto;
use std::ops::{Div, Mul};
use std::time::Duration;

/// Extension methods for Device to provide improved API
impl Device {
    /// The default output stream configuration tied to this device.
    ///
    /// This returns a `DeviceSupportedStreamConfig` that combines the device with its default
    /// output configuration, eliminating the need to pass the device separately when
    /// building streams.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// let stream = device.default_output_config()?
    ///     .on_alsa(|alsa| alsa.periods(2))
    ///     .build_output_stream::<f32, _, _>(
    ///         |data, _| { /* audio callback */ },
    ///         |err| eprintln!("Stream error: {}", err),
    ///         None,
    ///     )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn default_output_config(
        &self,
    ) -> Result<DeviceSupportedStreamConfig, DefaultStreamConfigError> {
        use crate::traits::DeviceTrait;
        let config = DeviceTrait::default_output_config(self)?;
        Ok(DeviceSupportedStreamConfig::new(self.clone(), config))
    }

    /// The default input stream configuration tied to this device.
    ///
    /// This returns a `DeviceSupportedStreamConfig` that combines the device with its default
    /// input configuration, eliminating the need to pass the device separately when
    /// building streams.
    pub fn default_input_config(
        &self,
    ) -> Result<DeviceSupportedStreamConfig, DefaultStreamConfigError> {
        use crate::traits::DeviceTrait;
        let config = DeviceTrait::default_input_config(self)?;
        Ok(DeviceSupportedStreamConfig::new(self.clone(), config))
    }
}

#[cfg(target_os = "emscripten")]
use wasm_bindgen::prelude::*;

mod error;
mod host;
pub mod platform;
mod samples_formats;
pub mod traits;

/// Platform-specific configurations and types.
///
/// This module provides access to platform-specific audio configuration
/// types that can be used with the builder pattern for fine-tuned control
/// over audio streams.
pub mod config {
    /// ALSA-specific configuration types.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ))]
    pub mod alsa {
        pub use crate::host::alsa::{AlsaAccessType, AlsaStreamConfig};
    }

    /// JACK-specific configuration types.
    #[cfg(all(
        feature = "jack",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows"
        )
    ))]
    pub mod jack {
        pub use crate::host::jack::JackStreamConfig;
    }
}

/// A host's device iterator yielding only *input* devices.
pub type InputDevices<I> = std::iter::Filter<I, fn(&<I as Iterator>::Item) -> bool>;

/// A host's device iterator yielding only *output* devices.
pub type OutputDevices<I> = std::iter::Filter<I, fn(&<I as Iterator>::Item) -> bool>;

/// Number of channels.
pub type ChannelCount = u16;

/// The number of samples processed per second for a single channel of audio.
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SampleRate(pub u32);

impl<T> Mul<T> for SampleRate
where
    u32: Mul<T, Output = u32>,
{
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        SampleRate(self.0 * rhs)
    }
}

impl<T> Div<T> for SampleRate
where
    u32: Div<T, Output = u32>,
{
    type Output = Self;

    fn div(self, rhs: T) -> Self {
        SampleRate(self.0 / rhs)
    }
}

/// The desired number of frames for the hardware buffer.
pub type FrameCount = u32;

/// The buffer size used by the device.
///
/// [`Default`] is used when no specific buffer size is set and uses the default
/// behavior of the given host. Note, the default buffer size may be surprisingly
/// large, leading to latency issues. If low latency is desired, [`Fixed(FrameCount)`]
/// should be used in accordance with the [`SupportedBufferSize`] range produced by
/// the [`SupportedStreamConfig`] API.
///
/// [`Default`]: BufferSize::Default
/// [`Fixed(FrameCount)`]: BufferSize::Fixed
/// [`SupportedStreamConfig`]: SupportedStreamConfig::buffer_size
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BufferSize {
    #[default]
    Default,
    Fixed(FrameCount),
}

#[cfg(target_os = "emscripten")]
impl wasm_bindgen::describe::WasmDescribe for BufferSize {
    fn describe() {}
}

#[cfg(target_os = "emscripten")]
impl wasm_bindgen::convert::IntoWasmAbi for BufferSize {
    type Abi = <Option<FrameCount> as wasm_bindgen::convert::IntoWasmAbi>::Abi;

    fn into_abi(self) -> Self::Abi {
        match self {
            Self::Default => None,
            Self::Fixed(fc) => Some(fc),
        }
        .into_abi()
    }
}

/// The set of parameters used to describe how to open a stream.
///
/// The sample format is omitted in favour of using a sample type.
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamConfig {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub buffer_size: BufferSize,
}

impl StreamConfig {
    /// Create a new `StreamConfig` with the given parameters.
    pub fn new(channels: ChannelCount, sample_rate: SampleRate, buffer_size: BufferSize) -> Self {
        Self {
            channels,
            sample_rate,
            buffer_size,
        }
    }

    /// Configure ALSA-specific options directly.
    ///
    /// This method provides direct access to ALSA configuration without requiring
    /// an explicit `.builder()` call. It's safe to call on all platforms - on
    /// non-ALSA platforms, the configuration is simply ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::{StreamConfig, SampleRate, BufferSize};
    /// let config = StreamConfig::new(2, SampleRate(44100), BufferSize::Default)
    ///     .on_alsa(|alsa| alsa.periods(2));
    /// ```
    pub fn on_alsa<F>(self, f: F) -> StreamConfigBuilder
    where
        F: FnOnce(AlsaStreamConfigWrapper) -> AlsaStreamConfigWrapper,
    {
        StreamConfigBuilder::from_stream_config(&self).on_alsa(f)
    }

    /// Configure JACK-specific options directly.
    ///
    /// This method provides direct access to JACK configuration without requiring
    /// an explicit `.builder()` call. It's safe to call on all platforms - on
    /// non-JACK platforms, the configuration is simply ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::{StreamConfig, SampleRate, BufferSize};
    /// let config = StreamConfig::new(2, SampleRate(44100), BufferSize::Default)
    ///     .on_jack(|jack| jack.client_name("my_app".to_string()));
    /// ```
    pub fn on_jack<F>(self, f: F) -> StreamConfigBuilder
    where
        F: FnOnce(JackStreamConfigWrapper) -> JackStreamConfigWrapper,
    {
        StreamConfigBuilder::from_stream_config(&self).on_jack(f)
    }

    /// Set buffer size configuration directly.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::{StreamConfig, SampleRate, BufferSize};
    /// let config = StreamConfig::new(2, SampleRate(44100), BufferSize::Default)
    ///     .with_buffer_size(BufferSize::Fixed(512));
    /// ```
    pub fn with_buffer_size(mut self, buffer_size: BufferSize) -> Self {
        self.buffer_size = buffer_size;
        self
    }

    /// Set the number of channels.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::{StreamConfig, SampleRate, BufferSize};
    /// let config = StreamConfig::new(2, SampleRate(44100), BufferSize::Default)
    ///     .with_channels(6); // 5.1 surround
    /// ```
    pub fn with_channels(mut self, channels: ChannelCount) -> Self {
        self.channels = channels;
        self
    }

    /// Set the sample rate.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::{StreamConfig, SampleRate, BufferSize};
    /// let config = StreamConfig::new(2, SampleRate(44100), BufferSize::Default)
    ///     .with_sample_rate(SampleRate(48000));
    /// ```
    pub fn with_sample_rate(mut self, sample_rate: SampleRate) -> Self {
        self.sample_rate = sample_rate;
        self
    }

    /// Build an output stream directly with typed samples.
    ///
    /// This is a convenience method that creates a builder internally and
    /// immediately builds an output stream. Note that you must specify the
    /// sample format since `StreamConfig` doesn't contain this information.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    /// # use cpal::{StreamConfig, SampleRate, BufferSize, SampleFormat};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// let config = StreamConfig::new(2, SampleRate(44100), BufferSize::Default);
    /// let stream = config.build_output_stream::<f32, _, _>(
    ///     &device,
    ///     SampleFormat::F32,
    ///     |data, _| {
    ///         for sample in data.iter_mut() {
    ///             *sample = 0.0;
    ///         }
    ///     },
    ///     |err| eprintln!("Stream error: {}", err),
    ///     None,
    /// )?;
    /// stream.play()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn build_output_stream<T, D, E>(
        self,
        device: &crate::Device,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        T: SizedSample,
        D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        StreamConfigBuilder::from_stream_config(&self)
            .sample_format(sample_format)
            .build_output_stream(device, data_callback, error_callback, timeout)
    }

    /// Build a raw output stream directly.
    ///
    /// This is a convenience method that creates a builder internally and
    /// immediately builds a raw output stream.
    pub fn build_output_stream_raw<D, E>(
        self,
        device: &crate::Device,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        StreamConfigBuilder::from_stream_config(&self)
            .sample_format(sample_format)
            .build_output_stream_raw(
                device,
                sample_format,
                data_callback,
                error_callback,
                timeout,
            )
    }

    /// Build an input stream directly with typed samples.
    ///
    /// This is a convenience method that creates a builder internally and
    /// immediately builds an input stream.
    pub fn build_input_stream<T, D, E>(
        self,
        device: &crate::Device,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        T: SizedSample,
        D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        StreamConfigBuilder::from_stream_config(&self)
            .sample_format(sample_format)
            .build_input_stream(device, data_callback, error_callback, timeout)
    }

    /// Build a raw input stream directly.
    ///
    /// This is a convenience method that creates a builder internally and
    /// immediately builds a raw input stream.
    pub fn build_input_stream_raw<D, E>(
        self,
        device: &crate::Device,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        StreamConfigBuilder::from_stream_config(&self)
            .sample_format(sample_format)
            .build_input_stream_raw(
                device,
                sample_format,
                data_callback,
                error_callback,
                timeout,
            )
    }
}

/// Builder for creating [`StreamConfig`] with platform-specific options.
///
/// This builder provides a **truly cross-platform** way to configure audio streams.
/// Platform-specific methods like `.on_alsa()` and `.on_jack()` work on **ALL platforms**
/// without any conditional compilation - they're simply no-ops where not supported.
///
/// Key advantages:
///
/// - **No arbitrary defaults**: Unlike `StreamConfig::default()`, the builder requires
///   you to explicitly set essential parameters or derive them from a supported configuration.
/// - **Sample format preservation**: When building from [`SupportedStreamConfig`], the
///   sample format is preserved, which is critical for correct stream creation.
/// - **True cross-platform compatibility**: Write once, run anywhere. No `#[cfg(...)]` needed.
/// - **Type safety**: The builder prevents creation of invalid configurations by requiring
///   all essential fields before building.
///
/// # Usage Patterns
///
/// ## 1. Building from SupportedStreamConfig (Recommended)
///
/// This is the preferred approach as it preserves device capabilities:
///
/// ```no_run
/// use cpal::traits::{DeviceTrait, HostTrait};
/// use cpal::BufferSize;
///
/// let host = cpal::default_host();
/// let device = host.default_output_device().unwrap();
/// let supported_config = device.default_output_config().unwrap();
///
/// // Create builder from supported config - preserves sample format!
/// let builder = supported_config.builder()
///     .buffer_size(BufferSize::Fixed(512));
///
/// let (config, platform_config) = builder.build();
/// ```
///
/// ## 2. Building from Scratch
///
/// Use this when you need specific parameters that may differ from device defaults:
///
/// ```no_run
/// use cpal::{StreamConfigBuilder, SampleRate, BufferSize, SampleFormat};
///
/// let builder = StreamConfigBuilder::new()
///     .channels(2)
///     .sample_rate(SampleRate(48_000))
///     .sample_format(SampleFormat::F32)
///     .buffer_size(BufferSize::Fixed(1024));
///
/// let (config, platform_config) = builder.build();
/// ```
///
/// ## 3. Platform-Specific Configuration
///
/// Set platform-specific options that are safely ignored on unsupported platforms:
///
/// ```no_run
/// # use cpal::traits::{DeviceTrait, HostTrait};
/// # let host = cpal::default_host();
/// # let device = host.default_output_device().unwrap();
/// # let supported_config = device.default_output_config().unwrap();
/// let builder = supported_config.builder()
///     .on_alsa(|alsa| alsa.periods(2))                    // Works on ALL platforms!
///     .on_jack(|jack| jack.client_name("MyApp".into()));  // Works on ALL platforms!
///
/// let (config, platform_config) = builder.build();
/// // No conditional compilation needed - works everywhere!
/// ```
///
/// ## 4. Direct Stream Creation
///
/// The builder can create streams directly, handling platform detection automatically:
///
/// ```no_run
/// # use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
/// # use std::time::Duration;
/// # let host = cpal::default_host();
/// # let device = host.default_output_device().unwrap();
/// # let supported_config = device.default_output_config().unwrap();
/// // This code works on ALL platforms - no conditional compilation needed!
/// let stream = supported_config.builder()
///     .on_alsa(|alsa| alsa.periods(2))                    // Works on ALL platforms!
///     .on_jack(|jack| jack.client_name("MyApp".into()))   // Works on ALL platforms!
///     .build_output_stream::<f32, _, _>(
///         &device,
///         |data, _info| { /* audio callback */ },
///         |err| eprintln!("Error: {}", err),
///         None
///     ).unwrap();
///
/// stream.play().unwrap();
/// ```
///
/// # Error Handling
///
/// The builder uses type-safe error handling:
///
/// ```no_run
/// use cpal::StreamConfigBuilder;
///
/// let incomplete = StreamConfigBuilder::new()
///     .channels(2); // Missing sample_rate and sample_format!
///
/// match incomplete.try_build() {
///     Some((config, platform_config)) => {
///         println!("Config: {:?}", config);
///     }
///     None => {
///         println!("Missing required configuration (sample_rate, sample_format)");
///     }
/// }
/// ```
#[derive(Clone, Debug, Default)]
pub struct StreamConfigBuilder {
    channels: Option<ChannelCount>,
    sample_rate: Option<SampleRate>,
    sample_format: Option<SampleFormat>,
    buffer_size: BufferSize,
    // Always include platform configs - they're just ignored on unsupported platforms
    alsa_config: Option<AlsaStreamConfigWrapper>,
    jack_config: Option<JackStreamConfigWrapper>,
    wasapi_config: Option<WasapiStreamConfigWrapper>,
}

/// Wrapper for ALSA configuration that exists on all platforms but only functions on ALSA platforms
#[derive(Clone, Debug, Default)]
pub struct AlsaStreamConfigWrapper {
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ))]
    pub(crate) inner: crate::host::alsa::AlsaStreamConfig,
}

/// Real implementation on ALSA platforms
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
))]
impl AlsaStreamConfigWrapper {
    pub fn periods(mut self, periods: u32) -> Self {
        self.inner.periods = Some(periods);
        self
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ))]
    pub fn access_type(mut self, access_type: crate::config::alsa::AlsaAccessType) -> Self {
        self.inner.access_type = Some(access_type);
        self
    }
}

/// Stub implementation on non-ALSA platforms
#[cfg(not(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
)))]
impl AlsaStreamConfigWrapper {
    pub fn periods(self, _periods: u32) -> Self {
        self // No-op on non-ALSA platforms
    }
}

/// Wrapper for JACK configuration that exists on all platforms but only functions on JACK platforms
#[derive(Clone, Debug, Default)]
pub struct JackStreamConfigWrapper {
    #[cfg(all(
        feature = "jack",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows"
        )
    ))]
    pub(crate) inner: crate::host::jack::JackStreamConfig,
}

/// Real implementation on JACK platforms
#[cfg(all(
    feature = "jack",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "macos",
        target_os = "windows"
    )
))]
impl JackStreamConfigWrapper {
    pub fn client_name(mut self, name: String) -> Self {
        self.inner.client_name = Some(name);
        self
    }

    pub fn connect_ports_automatically(mut self, connect: bool) -> Self {
        self.inner.connect_ports_automatically = Some(connect);
        self
    }

    pub fn start_server_automatically(mut self, start: bool) -> Self {
        self.inner.start_server_automatically = Some(start);
        self
    }
}

/// Stub implementation on non-JACK platforms
#[cfg(not(all(
    feature = "jack",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "macos",
        target_os = "windows"
    )
)))]
impl JackStreamConfigWrapper {
    pub fn client_name(self, _name: String) -> Self {
        self // No-op on non-JACK platforms
    }

    pub fn connect_ports_automatically(self, _connect: bool) -> Self {
        self // No-op on non-JACK platforms
    }

    pub fn start_server_automatically(self, _start: bool) -> Self {
        self // No-op on non-JACK platforms
    }
}

/// Platform-specific configuration bundle for stream creation.
///
/// This struct contains all platform-specific configurations that may be
/// relevant for the current platform. Only the configurations for the
/// active platform will be used.
#[derive(Clone, Debug, Default)]
pub struct PlatformStreamConfig {
    pub alsa: Option<AlsaStreamConfigWrapper>,
    pub jack: Option<JackStreamConfigWrapper>,
    pub wasapi: Option<WasapiStreamConfigWrapper>,
}

/// Wrapper for WASAPI configuration that exists on all platforms but only functions on Windows
#[derive(Clone, Debug, Default)]
pub struct WasapiStreamConfigWrapper {
    #[cfg(target_os = "windows")]
    pub(crate) inner: crate::host::wasapi::WasapiStreamConfig,
}

/// Real implementation on Windows
#[cfg(target_os = "windows")]
impl WasapiStreamConfigWrapper {
    pub fn exclusive_mode(mut self, exclusive: bool) -> Self {
        self.inner.exclusive_mode = Some(exclusive);
        self
    }
}

/// Stub implementation on non-Windows platforms
#[cfg(not(target_os = "windows"))]
impl WasapiStreamConfigWrapper {
    pub fn exclusive_mode(self, _exclusive: bool) -> Self {
        self // No-op on non-Windows platforms
    }
}

/// Describes the minimum and maximum supported buffer size for the device
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportedBufferSize {
    Range {
        min: FrameCount,
        max: FrameCount,
    },
    /// In the case that the platform provides no way of getting the default
    /// buffersize before starting a stream.
    Unknown,
}

/// Describes a range of supported stream configurations, retrieved via the
/// [`Device::supported_input/output_configs`](traits::DeviceTrait#required-methods) method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedStreamConfigRange {
    pub(crate) channels: ChannelCount,
    /// Minimum value for the samples rate of the supported formats.
    pub(crate) min_sample_rate: SampleRate,
    /// Maximum value for the samples rate of the supported formats.
    pub(crate) max_sample_rate: SampleRate,
    /// Buffersize ranges supported by the device
    pub(crate) buffer_size: SupportedBufferSize,
    /// Type of data expected by the device.
    pub(crate) sample_format: SampleFormat,
}

/// Describes a single supported stream configuration, retrieved via either a
/// [`SupportedStreamConfigRange`] instance or one of the
/// [`Device::default_input/output_config`](traits::DeviceTrait#required-methods) methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedStreamConfig {
    channels: ChannelCount,
    sample_rate: SampleRate,
    buffer_size: SupportedBufferSize,
    sample_format: SampleFormat,
}

/// A buffer of dynamically typed audio data, passed to raw stream callbacks.
///
/// Raw input stream callbacks receive `&Data`, while raw output stream callbacks expect `&mut
/// Data`.
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
#[derive(Debug)]
pub struct Data {
    data: *mut (),
    len: usize,
    sample_format: SampleFormat,
}

/// A monotonic time instance associated with a stream, retrieved from either:
///
/// 1. A timestamp provided to the stream's underlying audio data callback or
/// 2. The same time source used to generate timestamps for a stream's underlying audio data
///    callback.
///
/// `StreamInstant` represents a duration since some unspecified origin occurring either before
/// or equal to the moment the stream from which it was created begins.
///
/// ## Host `StreamInstant` Sources
///
/// | Host | Source |
/// | ---- | ------ |
/// | alsa | `snd_pcm_status_get_htstamp` |
/// | coreaudio | `mach_absolute_time` |
/// | wasapi | `QueryPerformanceCounter` |
/// | asio | `timeGetTime` |
/// | emscripten | `AudioContext.getOutputTimestamp` |
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct StreamInstant {
    secs: i64,
    nanos: u32,
}

/// A timestamp associated with a call to an input stream's data callback.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct InputStreamTimestamp {
    /// The instant the stream's data callback was invoked.
    pub callback: StreamInstant,
    /// The instant that data was captured from the device.
    ///
    /// E.g. The instant data was read from an ADC.
    pub capture: StreamInstant,
}

/// A timestamp associated with a call to an output stream's data callback.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct OutputStreamTimestamp {
    /// The instant the stream's data callback was invoked.
    pub callback: StreamInstant,
    /// The predicted instant that data written will be delivered to the device for playback.
    ///
    /// E.g. The instant data will be played by a DAC.
    pub playback: StreamInstant,
}

/// Information relevant to a single call to the user's input stream data callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputCallbackInfo {
    timestamp: InputStreamTimestamp,
}

/// Information relevant to a single call to the user's output stream data callback.
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputCallbackInfo {
    timestamp: OutputStreamTimestamp,
}

/// A stream configuration tied to a specific device.
///
/// This type combines a device with its supported stream configuration,
/// eliminating the need to pass the device separately when building streams.
/// It provides all the same configuration methods as `SupportedStreamConfig`
/// but with the device already captured.
#[derive(Clone)]
pub struct DeviceSupportedStreamConfig {
    device: crate::Device,
    config: SupportedStreamConfig,
}

impl DeviceSupportedStreamConfig {
    /// Create a new `DeviceSupportedStreamConfig` from a device and its configuration.
    pub fn new(device: crate::Device, config: SupportedStreamConfig) -> Self {
        Self { device, config }
    }

    /// Get a reference to the device.
    pub fn device(&self) -> &crate::Device {
        &self.device
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &SupportedStreamConfig {
        &self.config
    }

    /// Configure ALSA-specific options directly.
    ///
    /// This method provides direct access to ALSA configuration. It's safe to call
    /// on all platforms - on non-ALSA platforms, the configuration is simply ignored.
    pub fn on_alsa<F>(self, f: F) -> Self
    where
        F: FnOnce(AlsaStreamConfigWrapper) -> AlsaStreamConfigWrapper,
    {
        let builder = self.config.builder().on_alsa(f);
        let (new_config, _) = builder.build();
        // Convert back to SupportedStreamConfig
        let supported_config = SupportedStreamConfig::new(
            new_config.channels,
            new_config.sample_rate,
            self.config.buffer_size(),
            self.config.sample_format(),
        );
        Self {
            device: self.device,
            config: supported_config,
        }
    }

    /// Configure JACK-specific options directly.
    ///
    /// This method provides direct access to JACK configuration. It's safe to call
    /// on all platforms - on non-JACK platforms, the configuration is simply ignored.
    pub fn on_jack<F>(self, f: F) -> Self
    where
        F: FnOnce(JackStreamConfigWrapper) -> JackStreamConfigWrapper,
    {
        let builder = self.config.builder().on_jack(f);
        let (new_config, _) = builder.build();
        // Convert back to SupportedStreamConfig
        let supported_config = SupportedStreamConfig::new(
            new_config.channels,
            new_config.sample_rate,
            self.config.buffer_size(),
            self.config.sample_format(),
        );
        Self {
            device: self.device,
            config: supported_config,
        }
    }

    /// Set buffer size configuration directly.
    pub fn with_buffer_size(mut self, _buffer_size: BufferSize) -> Self {
        // Create a new config with the updated buffer size
        let supported_config = SupportedStreamConfig::new(
            self.config.channels(),
            self.config.sample_rate(),
            self.config.buffer_size(),
            self.config.sample_format(),
        );
        self.config = supported_config;
        self
    }

    /// Build an output stream directly with typed samples.
    ///
    /// This method builds a stream without requiring the device to be passed again.
    pub fn build_output_stream<T, DataCallback, ErrorCallback>(
        self,
        data_callback: DataCallback,
        error_callback: ErrorCallback,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        T: SizedSample,
        DataCallback: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(StreamError) + Send + 'static,
    {
        use crate::traits::DeviceTrait;
        self.device.build_output_stream(
            &self.config.config(),
            data_callback,
            error_callback,
            timeout,
        )
    }

    /// Build a raw output stream directly.
    ///
    /// This method builds a raw stream without requiring the device to be passed again.
    pub fn build_output_stream_raw<DataCallback, ErrorCallback>(
        self,
        sample_format: SampleFormat,
        data_callback: DataCallback,
        error_callback: ErrorCallback,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        DataCallback: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(StreamError) + Send + 'static,
    {
        use crate::traits::DeviceTrait;
        self.device.build_output_stream_raw(
            &self.config.config(),
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }

    /// Build an input stream directly with typed samples.
    pub fn build_input_stream<T, DataCallback, ErrorCallback>(
        self,
        data_callback: DataCallback,
        error_callback: ErrorCallback,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        T: SizedSample,
        DataCallback: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(StreamError) + Send + 'static,
    {
        use crate::traits::DeviceTrait;
        self.device.build_input_stream(
            &self.config.config(),
            data_callback,
            error_callback,
            timeout,
        )
    }

    /// Build a raw input stream directly.
    pub fn build_input_stream_raw<DataCallback, ErrorCallback>(
        self,
        sample_format: SampleFormat,
        data_callback: DataCallback,
        error_callback: ErrorCallback,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        DataCallback: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(StreamError) + Send + 'static,
    {
        use crate::traits::DeviceTrait;
        self.device.build_input_stream_raw(
            &self.config.config(),
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }

    /// Get the underlying builder for advanced configuration.
    pub fn builder(self) -> StreamConfigBuilder {
        self.config.builder()
    }

    /// Build the configuration and platform config.
    pub fn build(self) -> (StreamConfig, PlatformStreamConfig) {
        self.config.builder().build()
    }

    /// Get the channels count from the configuration.
    pub fn channels(&self) -> ChannelCount {
        self.config.channels()
    }

    /// Get the sample rate from the configuration.
    pub fn sample_rate(&self) -> SampleRate {
        self.config.sample_rate()
    }

    /// Get the buffer size from the configuration.
    pub fn buffer_size(&self) -> SupportedBufferSize {
        self.config.buffer_size()
    }

    /// Get the sample format from the configuration.
    pub fn sample_format(&self) -> SampleFormat {
        self.config.sample_format()
    }

    /// Get the basic StreamConfig.
    pub fn stream_config(&self) -> StreamConfig {
        self.config.config()
    }
}

impl From<DeviceSupportedStreamConfig> for StreamConfig {
    fn from(config: DeviceSupportedStreamConfig) -> Self {
        config.config.config()
    }
}

impl From<&DeviceSupportedStreamConfig> for StreamConfig {
    fn from(config: &DeviceSupportedStreamConfig) -> Self {
        config.config.config()
    }
}

impl From<DeviceSupportedStreamConfig> for SupportedStreamConfig {
    fn from(config: DeviceSupportedStreamConfig) -> Self {
        config.config
    }
}

impl From<&DeviceSupportedStreamConfig> for SupportedStreamConfig {
    fn from(config: &DeviceSupportedStreamConfig) -> Self {
        config.config.clone()
    }
}

impl std::fmt::Debug for DeviceSupportedStreamConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceSupportedStreamConfig")
            .field("config", &self.config)
            .finish()
    }
}

impl std::ops::Deref for DeviceSupportedStreamConfig {
    type Target = SupportedStreamConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl SupportedStreamConfig {
    pub fn new(
        channels: ChannelCount,
        sample_rate: SampleRate,
        buffer_size: SupportedBufferSize,
        sample_format: SampleFormat,
    ) -> Self {
        Self {
            channels,
            sample_rate,
            buffer_size,
            sample_format,
        }
    }

    pub fn channels(&self) -> ChannelCount {
        self.channels
    }

    pub fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    pub fn buffer_size(&self) -> SupportedBufferSize {
        self.buffer_size
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    pub fn config(&self) -> StreamConfig {
        StreamConfig {
            channels: self.channels,
            sample_rate: self.sample_rate,
            buffer_size: BufferSize::Default,
        }
    }

    /// Create a [`StreamConfigBuilder`] from this supported configuration.
    ///
    /// This is the recommended way to create a builder as it preserves all device
    /// capabilities including the sample format, which is critical for correct
    /// stream creation. The resulting builder can be further customized with
    /// buffer size settings and platform-specific options.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// let supported_config = device.default_output_config().unwrap();
    ///
    /// // Create builder with device-optimal settings
    /// let builder = supported_config.builder()
    ///     .buffer_size(cpal::BufferSize::Fixed(512))
    ///     .on_alsa(|alsa| alsa.periods(2));
    ///
    /// let (config, platform_config) = builder.build();
    /// ```
    pub fn builder(&self) -> StreamConfigBuilder {
        StreamConfigBuilder::from_supported_config(self)
    }

    /// Configure ALSA-specific options directly.
    ///
    /// This method provides direct access to ALSA configuration without requiring
    /// an explicit `.builder()` call. It's safe to call on all platforms - on
    /// non-ALSA platforms, the configuration is simply ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// let stream = device.default_output_config()?
    ///     .on_alsa(|alsa| alsa.periods(2))
    ///     .build_output_stream::<f32, _, _>(
    ///         |data, _| { /* audio callback */ },
    ///         |err| eprintln!("Stream error: {}", err),
    ///         None,
    ///     )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn on_alsa<F>(self, f: F) -> StreamConfigBuilder
    where
        F: FnOnce(AlsaStreamConfigWrapper) -> AlsaStreamConfigWrapper,
    {
        self.builder().on_alsa(f)
    }

    /// Configure JACK-specific options directly.
    ///
    /// This method provides direct access to JACK configuration without requiring
    /// an explicit `.builder()` call. It's safe to call on all platforms - on
    /// non-JACK platforms, the configuration is simply ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// let stream = device.default_output_config()?
    ///     .on_jack(|jack| jack.client_name("my_audio_app".to_string()))
    ///     .build_output_stream::<f32, _, _>(
    ///         |data, _| { /* audio callback */ },
    ///         |err| eprintln!("Stream error: {}", err),
    ///         None,
    ///     )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn on_jack<F>(self, f: F) -> StreamConfigBuilder
    where
        F: FnOnce(JackStreamConfigWrapper) -> JackStreamConfigWrapper,
    {
        self.builder().on_jack(f)
    }

    /// Set buffer size configuration directly.
    ///
    /// This method provides direct access to buffer size configuration without
    /// requiring an explicit `.builder()` call.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// let stream = device.default_output_config()?
    ///     .with_buffer_size(cpal::BufferSize::Fixed(512))
    ///     .build_output_stream::<f32, _, _>(
    ///         |data, _| { /* audio callback */ },
    ///         |err| eprintln!("Stream error: {}", err),
    ///         None,
    ///     )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_buffer_size(self, buffer_size: BufferSize) -> StreamConfigBuilder {
        self.builder().buffer_size(buffer_size)
    }

    /// Build an output stream directly with typed samples.
    ///
    /// This is a convenience method that creates a builder internally and
    /// immediately builds an output stream.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// let stream = device.default_output_config()?
    ///     .build_output_stream::<f32, _, _>(
    ///         |data, _| {
    ///             for sample in data.iter_mut() {
    ///                 *sample = 0.0;
    ///             }
    ///         },
    ///         |err| eprintln!("Stream error: {}", err),
    ///         None,
    ///     )?;
    /// stream.play()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn build_output_stream<T, D, E>(
        self,
        device: &crate::Device,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        T: SizedSample,
        D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.builder()
            .build_output_stream(device, data_callback, error_callback, timeout)
    }

    /// Build a raw output stream directly.
    ///
    /// This is a convenience method that creates a builder internally and
    /// immediately builds a raw output stream.
    pub fn build_output_stream_raw<D, E>(
        self,
        device: &crate::Device,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.builder().build_output_stream_raw(
            device,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }

    /// Build an input stream directly with typed samples.
    ///
    /// This is a convenience method that creates a builder internally and
    /// immediately builds an input stream.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    /// # let host = cpal::default_host();
    /// # let device = host.default_input_device().unwrap();
    /// let stream = device.default_input_config()?
    ///     .build_input_stream::<f32, _, _>(
    ///         |data, _| {
    ///             println!("Received {} samples", data.len());
    ///         },
    ///         |err| eprintln!("Stream error: {}", err),
    ///         None,
    ///     )?;
    /// stream.play()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn build_input_stream<T, D, E>(
        self,
        device: &crate::Device,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        T: SizedSample,
        D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.builder()
            .build_input_stream(device, data_callback, error_callback, timeout)
    }

    /// Build a raw input stream directly.
    ///
    /// This is a convenience method that creates a builder internally and
    /// immediately builds a raw input stream.
    pub fn build_input_stream_raw<D, E>(
        self,
        device: &crate::Device,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.builder().build_input_stream_raw(
            device,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }
}

impl StreamInstant {
    /// The amount of time elapsed from another instant to this one.
    ///
    /// Returns `None` if `earlier` is later than self.
    pub fn duration_since(&self, earlier: &Self) -> Option<Duration> {
        if self < earlier {
            None
        } else {
            (self.as_nanos() - earlier.as_nanos())
                .try_into()
                .ok()
                .map(Duration::from_nanos)
        }
    }

    /// Returns the instant in time after the given duration has passed.
    ///
    /// Returns `None` if the resulting instant would exceed the bounds of the underlying data
    /// structure.
    pub fn add(&self, duration: Duration) -> Option<Self> {
        self.as_nanos()
            .checked_add(duration.as_nanos() as i128)
            .and_then(Self::from_nanos_i128)
    }

    /// Returns the instant in time one `duration` ago.
    ///
    /// Returns `None` if the resulting instant would underflow. As a result, it is important to
    /// consider that on some platforms the [`StreamInstant`] may begin at `0` from the moment the
    /// source stream is created.
    pub fn sub(&self, duration: Duration) -> Option<Self> {
        self.as_nanos()
            .checked_sub(duration.as_nanos() as i128)
            .and_then(Self::from_nanos_i128)
    }

    fn as_nanos(&self) -> i128 {
        (self.secs as i128 * 1_000_000_000) + self.nanos as i128
    }

    #[allow(dead_code)]
    fn from_nanos(nanos: i64) -> Self {
        let secs = nanos / 1_000_000_000;
        let subsec_nanos = nanos - secs * 1_000_000_000;
        Self::new(secs, subsec_nanos as u32)
    }

    #[allow(dead_code)]
    fn from_nanos_i128(nanos: i128) -> Option<Self> {
        let secs = nanos / 1_000_000_000;
        if secs > i64::MAX as i128 || secs < i64::MIN as i128 {
            None
        } else {
            let subsec_nanos = nanos - secs * 1_000_000_000;
            debug_assert!(subsec_nanos < u32::MAX as i128);
            Some(Self::new(secs as i64, subsec_nanos as u32))
        }
    }

    #[allow(dead_code)]
    fn from_secs_f64(secs: f64) -> crate::StreamInstant {
        let s = secs.floor() as i64;
        let ns = ((secs - s as f64) * 1_000_000_000.0) as u32;
        Self::new(s, ns)
    }

    pub fn new(secs: i64, nanos: u32) -> Self {
        StreamInstant { secs, nanos }
    }
}

impl InputCallbackInfo {
    pub fn new(timestamp: InputStreamTimestamp) -> Self {
        Self { timestamp }
    }

    /// The timestamp associated with the call to an input stream's data callback.
    pub fn timestamp(&self) -> InputStreamTimestamp {
        self.timestamp
    }
}

impl OutputCallbackInfo {
    pub fn new(timestamp: OutputStreamTimestamp) -> Self {
        Self { timestamp }
    }

    /// The timestamp associated with the call to an output stream's data callback.
    pub fn timestamp(&self) -> OutputStreamTimestamp {
        self.timestamp
    }
}

#[allow(clippy::len_without_is_empty)]
impl Data {
    // Internal constructor for host implementations to use.
    //
    // The following requirements must be met in order for the safety of `Data`'s public API.
    //
    // - The `data` pointer must point to the first sample in the slice containing all samples.
    // - The `len` must describe the length of the buffer as a number of samples in the expected
    //   format specified via the `sample_format` argument.
    // - The `sample_format` must correctly represent the underlying sample data delivered/expected
    //   by the stream.
    pub(crate) unsafe fn from_parts(
        data: *mut (),
        len: usize,
        sample_format: SampleFormat,
    ) -> Self {
        Data {
            data,
            len,
            sample_format,
        }
    }

    /// The sample format of the internal audio data.
    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    /// The full length of the buffer in samples.
    ///
    /// The returned length is the same length as the slice of type `T` that would be returned via
    /// [`as_slice`](Self::as_slice) given a sample type that matches the inner sample format.
    pub fn len(&self) -> usize {
        self.len
    }

    /// The raw slice of memory representing the underlying audio data as a slice of bytes.
    ///
    /// It is up to the user to interpret the slice of memory based on [`Data::sample_format`].
    pub fn bytes(&self) -> &[u8] {
        let len = self.len * self.sample_format.sample_size();
        // The safety of this block relies on correct construction of the `Data` instance.
        // See the unsafe `from_parts` constructor for these requirements.
        unsafe { std::slice::from_raw_parts(self.data as *const u8, len) }
    }

    /// The raw slice of memory representing the underlying audio data as a slice of bytes.
    ///
    /// It is up to the user to interpret the slice of memory based on [`Data::sample_format`].
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        let len = self.len * self.sample_format.sample_size();
        // The safety of this block relies on correct construction of the `Data` instance. See
        // the unsafe `from_parts` constructor for these requirements.
        unsafe { std::slice::from_raw_parts_mut(self.data as *mut u8, len) }
    }

    /// Access the data as a slice of sample type `T`.
    ///
    /// Returns `None` if the sample type does not match the expected sample format.
    pub fn as_slice<T>(&self) -> Option<&[T]>
    where
        T: SizedSample,
    {
        if T::FORMAT == self.sample_format {
            // The safety of this block relies on correct construction of the `Data` instance. See
            // the unsafe `from_parts` constructor for these requirements.
            unsafe { Some(std::slice::from_raw_parts(self.data as *const T, self.len)) }
        } else {
            None
        }
    }

    /// Access the data as a slice of sample type `T`.
    ///
    /// Returns `None` if the sample type does not match the expected sample format.
    pub fn as_slice_mut<T>(&mut self) -> Option<&mut [T]>
    where
        T: SizedSample,
    {
        if T::FORMAT == self.sample_format {
            // The safety of this block relies on correct construction of the `Data` instance. See
            // the unsafe `from_parts` constructor for these requirements.
            unsafe {
                Some(std::slice::from_raw_parts_mut(
                    self.data as *mut T,
                    self.len,
                ))
            }
        } else {
            None
        }
    }
}

impl SupportedStreamConfigRange {
    pub fn new(
        channels: ChannelCount,
        min_sample_rate: SampleRate,
        max_sample_rate: SampleRate,
        buffer_size: SupportedBufferSize,
        sample_format: SampleFormat,
    ) -> Self {
        Self {
            channels,
            min_sample_rate,
            max_sample_rate,
            buffer_size,
            sample_format,
        }
    }

    pub fn channels(&self) -> ChannelCount {
        self.channels
    }

    pub fn min_sample_rate(&self) -> SampleRate {
        self.min_sample_rate
    }

    pub fn max_sample_rate(&self) -> SampleRate {
        self.max_sample_rate
    }

    pub fn buffer_size(&self) -> &SupportedBufferSize {
        &self.buffer_size
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    /// Retrieve a [`SupportedStreamConfig`] with the given sample rate and buffer size.
    ///
    /// # Panics
    ///
    /// Panics if the given `sample_rate` is outside the range specified within
    /// this [`SupportedStreamConfigRange`] instance. For a non-panicking
    /// variant, use [`try_with_sample_rate`](#method.try_with_sample_rate).
    pub fn with_sample_rate(self, sample_rate: SampleRate) -> SupportedStreamConfig {
        self.try_with_sample_rate(sample_rate)
            .expect("sample rate out of range")
    }

    /// Retrieve a [`SupportedStreamConfig`] with the given sample rate and buffer size.
    ///
    /// Returns `None` if the given sample rate is outside the range specified
    /// within this [`SupportedStreamConfigRange`] instance.
    pub fn try_with_sample_rate(self, sample_rate: SampleRate) -> Option<SupportedStreamConfig> {
        if self.min_sample_rate <= sample_rate && sample_rate <= self.max_sample_rate {
            Some(SupportedStreamConfig {
                channels: self.channels,
                sample_rate,
                sample_format: self.sample_format,
                buffer_size: self.buffer_size,
            })
        } else {
            None
        }
    }

    /// Turns this [`SupportedStreamConfigRange`] into a [`SupportedStreamConfig`] corresponding to the maximum samples rate.
    #[inline]
    pub fn with_max_sample_rate(self) -> SupportedStreamConfig {
        SupportedStreamConfig {
            channels: self.channels,
            sample_rate: self.max_sample_rate,
            sample_format: self.sample_format,
            buffer_size: self.buffer_size,
        }
    }

    /// A comparison function which compares two [`SupportedStreamConfigRange`]s in terms of their priority of
    /// use as a default stream format.
    ///
    /// Some backends do not provide a default stream format for their audio devices. In these
    /// cases, CPAL attempts to decide on a reasonable default format for the user. To do this we
    /// use the "greatest" of all supported stream formats when compared with this method.
    ///
    /// SupportedStreamConfigs are prioritised by the following heuristics:
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

        let cmp_f32 = (self.sample_format == F32).cmp(&(other.sample_format == F32));
        if cmp_f32 != Equal {
            return cmp_f32;
        }

        let cmp_i16 = (self.sample_format == I16).cmp(&(other.sample_format == I16));
        if cmp_i16 != Equal {
            return cmp_i16;
        }

        let cmp_u16 = (self.sample_format == U16).cmp(&(other.sample_format == U16));
        if cmp_u16 != Equal {
            return cmp_u16;
        }

        const HZ_44100: SampleRate = SampleRate(44_100);
        let r44100_in_self = self.min_sample_rate <= HZ_44100 && HZ_44100 <= self.max_sample_rate;
        let r44100_in_other =
            other.min_sample_rate <= HZ_44100 && HZ_44100 <= other.max_sample_rate;
        let cmp_r44100 = r44100_in_self.cmp(&r44100_in_other);
        if cmp_r44100 != Equal {
            return cmp_r44100;
        }

        self.max_sample_rate.cmp(&other.max_sample_rate)
    }
}

#[test]
fn test_cmp_default_heuristics() {
    let mut formats = [
        SupportedStreamConfigRange {
            buffer_size: SupportedBufferSize::Range { min: 256, max: 512 },
            channels: 2,
            min_sample_rate: SampleRate(1),
            max_sample_rate: SampleRate(96000),
            sample_format: SampleFormat::F32,
        },
        SupportedStreamConfigRange {
            buffer_size: SupportedBufferSize::Range { min: 256, max: 512 },
            channels: 1,
            min_sample_rate: SampleRate(1),
            max_sample_rate: SampleRate(96000),
            sample_format: SampleFormat::F32,
        },
        SupportedStreamConfigRange {
            buffer_size: SupportedBufferSize::Range { min: 256, max: 512 },
            channels: 2,
            min_sample_rate: SampleRate(1),
            max_sample_rate: SampleRate(96000),
            sample_format: SampleFormat::I16,
        },
        SupportedStreamConfigRange {
            buffer_size: SupportedBufferSize::Range { min: 256, max: 512 },
            channels: 2,
            min_sample_rate: SampleRate(1),
            max_sample_rate: SampleRate(96000),
            sample_format: SampleFormat::U16,
        },
        SupportedStreamConfigRange {
            buffer_size: SupportedBufferSize::Range { min: 256, max: 512 },
            channels: 2,
            min_sample_rate: SampleRate(1),
            max_sample_rate: SampleRate(22050),
            sample_format: SampleFormat::F32,
        },
    ];

    formats.sort_by(|a, b| a.cmp_default_heuristics(b));

    // lowest-priority first:
    assert_eq!(formats[0].sample_format(), SampleFormat::F32);
    assert_eq!(formats[0].min_sample_rate(), SampleRate(1));
    assert_eq!(formats[0].max_sample_rate(), SampleRate(96000));
    assert_eq!(formats[0].channels(), 1);

    assert_eq!(formats[1].sample_format(), SampleFormat::U16);
    assert_eq!(formats[1].min_sample_rate(), SampleRate(1));
    assert_eq!(formats[1].max_sample_rate(), SampleRate(96000));
    assert_eq!(formats[1].channels(), 2);

    assert_eq!(formats[2].sample_format(), SampleFormat::I16);
    assert_eq!(formats[2].min_sample_rate(), SampleRate(1));
    assert_eq!(formats[2].max_sample_rate(), SampleRate(96000));
    assert_eq!(formats[2].channels(), 2);

    assert_eq!(formats[3].sample_format(), SampleFormat::F32);
    assert_eq!(formats[3].min_sample_rate(), SampleRate(1));
    assert_eq!(formats[3].max_sample_rate(), SampleRate(22050));
    assert_eq!(formats[3].channels(), 2);

    assert_eq!(formats[4].sample_format(), SampleFormat::F32);
    assert_eq!(formats[4].min_sample_rate(), SampleRate(1));
    assert_eq!(formats[4].max_sample_rate(), SampleRate(96000));
    assert_eq!(formats[4].channels(), 2);
}

impl From<SupportedStreamConfig> for StreamConfig {
    fn from(conf: SupportedStreamConfig) -> Self {
        conf.config()
    }
}

impl StreamConfigBuilder {
    /// Create a new builder with no defaults set.
    ///
    /// When using this constructor, you must call [`channels`](Self::channels),
    /// [`sample_rate`](Self::sample_rate), and [`sample_format`](Self::sample_format)
    /// before calling [`build`](Self::build) or the build will panic.
    ///
    /// # Recommendation
    ///
    /// Consider using [`SupportedStreamConfig::builder`] instead, which automatically
    /// sets appropriate values based on device capabilities and preserves the sample format.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cpal::{StreamConfigBuilder, SampleRate, SampleFormat, BufferSize};
    ///
    /// let builder = StreamConfigBuilder::new()
    ///     .channels(2)
    ///     .sample_rate(SampleRate(44_100))
    ///     .sample_format(SampleFormat::F32)
    ///     .buffer_size(BufferSize::Default);
    ///
    /// let (config, platform_config) = builder.build();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a builder from a [`SupportedStreamConfig`].
    ///
    /// This is the recommended way to create a builder as it preserves all the
    /// essential configuration including sample format, which is critical for
    /// correct stream creation. All device capabilities are preserved and you
    /// can further customize buffer size and platform-specific options.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait};
    /// # use cpal::{BufferSize, StreamConfigBuilder};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// let supported_config = device.default_output_config().unwrap();
    ///
    /// let builder = StreamConfigBuilder::from_supported_config(&supported_config)
    ///     .buffer_size(BufferSize::Fixed(1024));
    /// ```
    pub fn from_supported_config(config: &SupportedStreamConfig) -> Self {
        Self {
            channels: Some(config.channels()),
            sample_rate: Some(config.sample_rate()),
            sample_format: Some(config.sample_format()),
            buffer_size: BufferSize::Default,
            alsa_config: None,
            jack_config: None,
            wasapi_config: None,
        }
    }

    /// Create a builder from a [`StreamConfig`].
    ///
    /// This creates a builder from an existing stream configuration, allowing
    /// you to add platform-specific options or modify settings.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::{StreamConfig, StreamConfigBuilder, SampleRate, BufferSize, SampleFormat};
    /// let config = StreamConfig::new(2, SampleRate(44100), BufferSize::Default);
    /// let builder = StreamConfigBuilder::from_stream_config(&config)
    ///     .sample_format(SampleFormat::F32);
    /// ```
    pub fn from_stream_config(config: &StreamConfig) -> Self {
        Self {
            channels: Some(config.channels),
            sample_rate: Some(config.sample_rate),
            sample_format: None, // StreamConfig doesn't contain sample format
            buffer_size: config.buffer_size,
            alsa_config: None,
            jack_config: None,
            wasapi_config: None,
        }
    }

    /// Set the number of channels.
    pub fn channels(mut self, channels: ChannelCount) -> Self {
        self.channels = Some(channels);
        self
    }

    /// Set the sample rate.
    pub fn sample_rate(mut self, sample_rate: SampleRate) -> Self {
        self.sample_rate = Some(sample_rate);
        self
    }

    /// Set the sample format.
    pub fn sample_format(mut self, sample_format: SampleFormat) -> Self {
        self.sample_format = Some(sample_format);
        self
    }

    /// Set the buffer size.
    pub fn buffer_size(mut self, buffer_size: BufferSize) -> Self {
        self.buffer_size = buffer_size;
        self
    }

    /// Configure ALSA-specific options.
    ///
    /// **This method works on ALL platforms** without any conditional compilation needed.
    /// On Linux/BSD systems with ALSA, the configuration takes effect. On other platforms,
    /// it's safely ignored (no-op). No `#[cfg(...)]` attributes required!
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cpal::StreamConfigBuilder;
    /// // This code works everywhere - no platform checks needed!
    /// let builder = StreamConfigBuilder::new()
    ///     .on_alsa(|alsa| alsa.periods(2)); // Safe on ALL platforms
    /// ```
    pub fn on_alsa<F>(mut self, f: F) -> Self
    where
        F: FnOnce(AlsaStreamConfigWrapper) -> AlsaStreamConfigWrapper,
    {
        let config = self.alsa_config.unwrap_or_default();
        self.alsa_config = Some(f(config));
        self
    }

    /// Configure JACK-specific options.
    ///
    /// **This method works on ALL platforms** without any conditional compilation needed.
    /// On systems with JACK installed and the jack feature enabled, the configuration
    /// takes effect. On platforms without JACK support, this configuration is safely ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cpal::StreamConfigBuilder;
    /// let builder = StreamConfigBuilder::new()
    ///     .on_jack(|jack| jack.client_name("my_audio_app".to_string())); // Safe on all platforms
    /// ```
    pub fn on_jack<F>(mut self, f: F) -> Self
    where
        F: FnOnce(JackStreamConfigWrapper) -> JackStreamConfigWrapper,
    {
        let config = self.jack_config.unwrap_or_default();
        self.jack_config = Some(f(config));
        self
    }

    /// Configure WASAPI-specific options.
    ///
    /// **This method works on ALL platforms** without any conditional compilation needed.
    /// On Windows systems, the configuration takes effect. On other platforms,
    /// this configuration is safely ignored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cpal::StreamConfigBuilder;
    /// let builder = StreamConfigBuilder::new()
    ///     .on_wasapi(|wasapi| wasapi.exclusive_mode(true)); // Safe on all platforms
    /// ```
    pub fn on_wasapi<F>(mut self, f: F) -> Self
    where
        F: FnOnce(WasapiStreamConfigWrapper) -> WasapiStreamConfigWrapper,
    {
        let config = self.wasapi_config.unwrap_or_default();
        self.wasapi_config = Some(f(config));
        self
    }

    /// Build the stream configuration.
    ///
    /// Returns a tuple of (`StreamConfig`, `PlatformStreamConfig`) that can be
    /// used to create streams or passed to platform-specific build methods.
    /// The `StreamConfig` contains the standard audio parameters, while
    /// `PlatformStreamConfig` contains any platform-specific options.
    ///
    /// # Panics
    ///
    /// Panics if any of the required fields (channels, sample_rate, sample_format)
    /// have not been set. Use [`try_build`](Self::try_build) for a non-panicking version.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cpal::traits::{DeviceTrait, HostTrait};
    /// # let host = cpal::default_host();
    /// # let device = host.default_output_device().unwrap();
    /// # let supported_config = device.default_output_config().unwrap();
    /// let builder = supported_config.builder();
    /// let (stream_config, platform_config) = builder.build();
    ///
    /// println!("Channels: {}", stream_config.channels);
    /// println!("Sample rate: {}", stream_config.sample_rate.0);
    /// ```
    pub fn build(self) -> (StreamConfig, PlatformStreamConfig) {
        self.try_build()
            .expect("StreamConfigBuilder is missing required fields")
    }

    /// Try to build the stream configuration.
    ///
    /// Returns `None` if any required fields (channels, sample_rate, sample_format)
    /// are missing. This is the safe alternative to [`build`](Self::build) that
    /// doesn't panic.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cpal::StreamConfigBuilder;
    ///
    /// let incomplete = StreamConfigBuilder::new().channels(2);
    ///
    /// match incomplete.try_build() {
    ///     Some((config, platform_config)) => {
    ///         println!("Built successfully: {:?}", config);
    ///     }
    ///     None => {
    ///         println!("Missing required configuration (sample_rate, sample_format)");
    ///     }
    /// }
    /// ```
    pub fn try_build(self) -> Option<(StreamConfig, PlatformStreamConfig)> {
        let channels = self.channels?;
        let sample_rate = self.sample_rate?;
        let _sample_format = self.sample_format?;

        let stream_config = StreamConfig {
            channels,
            sample_rate,
            buffer_size: self.buffer_size,
        };

        let platform_config = PlatformStreamConfig {
            alsa: self.alsa_config,
            jack: self.jack_config,
            wasapi: self.wasapi_config,
        };

        Some((stream_config, platform_config))
    }

    /// Build an input stream using this configuration.
    ///
    /// This is a convenience method that handles platform-specific configuration
    /// automatically. It detects the active audio backend at runtime and applies
    /// the appropriate platform-specific settings, falling back to standard stream
    /// creation if no platform-specific configuration is set.
    pub fn build_input_stream<T, D, E>(
        self,
        device: &crate::Device,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        T: SizedSample,
        D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let sample_format = self
            .sample_format
            .expect("sample_format must be set before building stream");

        self.build_input_stream_raw(
            device,
            sample_format,
            move |data, info| {
                data_callback(
                    data.as_slice()
                        .expect("host supplied incorrect sample type"),
                    info,
                )
            },
            error_callback,
            timeout,
        )
    }

    /// Build a raw input stream using this configuration.
    ///
    /// This is a convenience method that handles platform-specific configuration
    /// automatically. Unlike [`build_input_stream`](Self::build_input_stream), this
    /// method works with dynamically typed audio data and requires you to specify
    /// the sample format explicitly.
    pub fn build_input_stream_raw<D, E>(
        self,
        device: &crate::Device,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let (config, platform_config) = self.build();

        // Try platform-specific methods first, then fall back to standard method
        Self::build_input_stream_with_platform_config(
            device,
            &config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
            &platform_config,
        )
    }

    /// Build an output stream using this configuration.
    ///
    /// This is a convenience method that handles platform-specific configuration
    /// automatically. It detects the active audio backend at runtime and applies
    /// the appropriate platform-specific settings, falling back to standard stream
    /// creation if no platform-specific configuration is set.
    pub fn build_output_stream<T, D, E>(
        self,
        device: &crate::Device,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        T: SizedSample,
        D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let sample_format = self
            .sample_format
            .expect("sample_format must be set before building stream");

        self.build_output_stream_raw(
            device,
            sample_format,
            move |data, info| {
                data_callback(
                    data.as_slice_mut()
                        .expect("host supplied incorrect sample type"),
                    info,
                )
            },
            error_callback,
            timeout,
        )
    }

    /// Build a raw output stream using this configuration.
    ///
    /// This is a convenience method that handles platform-specific configuration
    /// automatically.
    pub fn build_output_stream_raw<D, E>(
        self,
        device: &crate::Device,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let (config, platform_config) = self.build();

        // Try platform-specific methods first, then fall back to standard method
        Self::build_output_stream_with_platform_config(
            device,
            &config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
            &platform_config,
        )
    }

    // Helper methods for platform-specific stream creation

    fn build_input_stream_with_platform_config<D, E>(
        device: &crate::Device,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
        _platform_config: &PlatformStreamConfig,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // Try ALSA-specific configuration first
        if let Some(_alsa_config_wrapper) = &_platform_config.alsa {
            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd"
            ))]
            {
                match device.as_inner() {
                    crate::platform::DeviceInner::Alsa(alsa_device) => {
                        return alsa_device
                            .build_input_stream_raw_with_alsa_config(
                                config,
                                sample_format,
                                data_callback,
                                error_callback,
                                timeout,
                                Some(&_alsa_config_wrapper.inner),
                            )
                            .map(crate::Stream::from);
                    }
                    _ => {} // Not ALSA device, continue to standard method
                }
            }
        }

        // Try JACK-specific configuration
        if let Some(_jack_config_wrapper) = &_platform_config.jack {
            #[cfg(all(
                feature = "jack",
                any(
                    target_os = "linux",
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "netbsd",
                    target_os = "macos",
                    target_os = "windows"
                )
            ))]
            {
                match device.as_inner() {
                    crate::platform::DeviceInner::Jack(jack_device) => {
                        return jack_device
                            .build_input_stream_raw_with_jack_config(
                                config,
                                sample_format,
                                data_callback,
                                error_callback,
                                timeout,
                                Some(&_jack_config_wrapper.inner),
                            )
                            .map(crate::Stream::from);
                    }
                    _ => {} // Not JACK device, continue to standard method
                }
            }
        }

        // Try WASAPI-specific configuration
        if let Some(_wasapi_config_wrapper) = &_platform_config.wasapi {
            #[cfg(target_os = "windows")]
            {
                match device.as_inner() {
                    crate::platform::DeviceInner::Wasapi(wasapi_device) => {
                        return wasapi_device
                            .build_input_stream_raw_with_wasapi_config(
                                config,
                                sample_format,
                                data_callback,
                                error_callback,
                                timeout,
                                Some(&_wasapi_config_wrapper.inner),
                            )
                            .map(crate::Stream::from);
                    }
                    #[cfg(any(feature = "asio", feature = "jack"))]
                    _ => {} // Not WASAPI device, continue to standard method
                }
            }
        }

        // Fall back to standard method
        use crate::traits::DeviceTrait;
        device.build_input_stream_raw(
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }

    fn build_output_stream_with_platform_config<D, E>(
        device: &crate::Device,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
        _platform_config: &PlatformStreamConfig,
    ) -> Result<crate::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // Try ALSA-specific configuration first
        if let Some(_alsa_config_wrapper) = &_platform_config.alsa {
            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd"
            ))]
            {
                match device.as_inner() {
                    crate::platform::DeviceInner::Alsa(alsa_device) => {
                        return alsa_device
                            .build_output_stream_raw_with_alsa_config(
                                config,
                                sample_format,
                                data_callback,
                                error_callback,
                                timeout,
                                Some(&_alsa_config_wrapper.inner),
                            )
                            .map(crate::Stream::from);
                    }
                    _ => {} // Not ALSA device, continue to standard method
                }
            }
        }

        // Try JACK-specific configuration
        if let Some(_jack_config_wrapper) = &_platform_config.jack {
            #[cfg(all(
                feature = "jack",
                any(
                    target_os = "linux",
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "netbsd",
                    target_os = "macos",
                    target_os = "windows"
                )
            ))]
            {
                match device.as_inner() {
                    crate::platform::DeviceInner::Jack(jack_device) => {
                        return jack_device
                            .build_output_stream_raw_with_jack_config(
                                config,
                                sample_format,
                                data_callback,
                                error_callback,
                                timeout,
                                Some(&_jack_config_wrapper.inner),
                            )
                            .map(crate::Stream::from);
                    }
                    _ => {} // Not JACK device, continue to standard method
                }
            }
        }

        // Try WASAPI-specific configuration
        if let Some(_wasapi_config_wrapper) = &_platform_config.wasapi {
            #[cfg(target_os = "windows")]
            {
                match device.as_inner() {
                    crate::platform::DeviceInner::Wasapi(wasapi_device) => {
                        return wasapi_device
                            .build_output_stream_raw_with_wasapi_config(
                                config,
                                sample_format,
                                data_callback,
                                error_callback,
                                timeout,
                                Some(&_wasapi_config_wrapper.inner),
                            )
                            .map(crate::Stream::from);
                    }
                    #[cfg(any(feature = "asio", feature = "jack"))]
                    _ => {} // Not WASAPI device, continue to standard method
                }
            }
        }

        // Fall back to standard method
        use crate::traits::DeviceTrait;
        device.build_output_stream_raw(
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }
}

// If a backend does not provide an API for retrieving supported formats, we query it with a bunch
// of commonly used rates. This is always the case for wasapi and is sometimes the case for alsa.
//
// If a rate you desire is missing from this list, feel free to add it!
#[cfg(target_os = "windows")]
const COMMON_SAMPLE_RATES: &[SampleRate] = &[
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
    SampleRate(384000),
];

#[test]
fn test_stream_instant() {
    let a = StreamInstant::new(2, 0);
    let b = StreamInstant::new(-2, 0);
    let min = StreamInstant::new(i64::MIN, 0);
    let max = StreamInstant::new(i64::MAX, 0);

    assert_eq!(
        a.sub(Duration::from_secs(1)),
        Some(StreamInstant::new(1, 0))
    );
    assert_eq!(
        a.sub(Duration::from_secs(2)),
        Some(StreamInstant::new(0, 0))
    );
    assert_eq!(
        a.sub(Duration::from_secs(3)),
        Some(StreamInstant::new(-1, 0))
    );
    assert_eq!(min.sub(Duration::from_secs(1)), None);

    assert_eq!(
        b.add(Duration::from_secs(1)),
        Some(StreamInstant::new(-1, 0))
    );
    assert_eq!(
        b.add(Duration::from_secs(2)),
        Some(StreamInstant::new(0, 0))
    );
    assert_eq!(
        b.add(Duration::from_secs(3)),
        Some(StreamInstant::new(1, 0))
    );
    assert_eq!(max.add(Duration::from_secs(1)), None);
}

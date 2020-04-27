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
//! Before we can create a stream, we must decide what the configuration of the audio stream is
//! going to be. You can query all the supported configurations with the
//! `supported_input_configs()` and `supported_output_configs()` methods. These produce a list of
//! `SupportedStreamConfigRange` structs which can later be turned into actual
//! `SupportedStreamConfig` structs. If you don't want to query the list of configs, you can also
//! build your own `StreamConfig` manually, but doing so could lead to an error when building the
//! stream if the config is not supported by the device.
//!
//! > **Note**: the `supported_input/output_configs()` methods could return an error for example if
//! > the device has been disconnected.
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
//! Now that we have everything for the stream, we are ready to create it from our selected device:
//!
//! ```no_run
//! use cpal::Data;
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let config = device.default_output_config().unwrap().into();
//! let stream = device.build_output_stream(
//!     &config,
//!     move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
//!         // react to stream events and read or write stream data here.
//!     },
//!     move |err| {
//!         // react to errors here.
//!     },
//! );
//! ```
//!
//! While the stream is running, the selected audio device will periodically call the data callback
//! that was passed to the function. The callback is passed an instance of either `&Data` or
//! `&mut Data` depending on whether the stream is an input stream or output stream respectively.
//!
//! > **Note**: Creating and running a stream will *not* block the thread. On modern platforms, the
//! > given callback is called by a dedicated, high-priority thread responsible for delivering
//! > audio data to the system's audio device in a timely manner. On older platforms that only
//! > provide a blocking API (e.g. ALSA), CPAL will create a thread in order to consistently
//! > provide non-blocking behaviour (currently this is a thread per stream, but this may change to
//! > use a single thread for all streams). *If this is an issue for your platform or design,
//! > please share your issue and use-case with the CPAL team on the github issue tracker for
//! > consideration.*
//!
//! In this example, we simply fill the given output buffer with silence.
//!
//! ```no_run
//! use cpal::{Data, Sample, SampleFormat};
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let supported_config = device.default_output_config().unwrap();
//! let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);
//! let sample_format = supported_config.sample_format();
//! let config = supported_config.into();
//! let stream = match sample_format {
//!     SampleFormat::F32 => device.build_output_stream(&config, write_silence::<f32>, err_fn),
//!     SampleFormat::I16 => device.build_output_stream(&config, write_silence::<i16>, err_fn),
//!     SampleFormat::U16 => device.build_output_stream(&config, write_silence::<u16>, err_fn),
//! }.unwrap();
//!
//! fn write_silence<T: Sample>(data: &mut [T], _: &cpal::OutputCallbackInfo) {
//!     for sample in data.iter_mut() {
//!         *sample = Sample::from(&0.0);
//!     }
//! }
//! ```
//!
//! Not all platforms automatically run the stream upon creation. To ensure the stream has started,
//! we can use `Stream::play`.
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
//! # let stream = device.build_output_stream_raw(&config, sample_format, data_fn, err_fn).unwrap();
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
//! # let stream = device.build_output_stream_raw(&config, sample_format, data_fn, err_fn).unwrap();
//! stream.pause().unwrap();
//! ```

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
    available_hosts, default_host, host_from_id, Device, Devices, Host, HostId, Stream,
    SupportedInputConfigs, SupportedOutputConfigs, ALL_HOSTS,
};
pub use samples_formats::{Sample, SampleFormat};

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

/// The set of parameters used to describe how to open a stream.
///
/// The sample format is omitted in favour of using a sample type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamConfig {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
}

/// Describes a range of supported stream configurations, retrieved via the
/// `Device::supported_input/output_configs` method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedStreamConfigRange {
    pub(crate) channels: ChannelCount,
    /// Minimum value for the samples rate of the supported formats.
    pub(crate) min_sample_rate: SampleRate,
    /// Maximum value for the samples rate of the supported formats.
    pub(crate) max_sample_rate: SampleRate,
    /// Type of data expected by the device.
    pub(crate) sample_format: SampleFormat,
}

/// Describes a single supported stream configuration, retrieved via either a
/// `SupportedStreamConfigRange` instance or one of the `Device::default_input/output_config` methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedStreamConfig {
    channels: ChannelCount,
    sample_rate: SampleRate,
    sample_format: SampleFormat,
}

/// A buffer of dynamically typed audio data, passed to raw stream callbacks.
///
/// Raw input stream callbacks receive `&Data`, while raw output stream callbacks expect `&mut
/// Data`.
#[derive(Debug)]
pub struct Data {
    data: *mut (),
    len: usize,
    sample_format: SampleFormat,
}

/// Information relevant to a single call to the user's output stream data callback.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputCallbackInfo {}

/// Information relevant to a single call to the user's input stream data callback.
#[derive(Debug, Clone, PartialEq)]
pub struct InputCallbackInfo {}

impl SupportedStreamConfig {
    pub fn channels(&self) -> ChannelCount {
        self.channels
    }

    pub fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    pub fn config(&self) -> StreamConfig {
        StreamConfig {
            channels: self.channels,
            sample_rate: self.sample_rate,
        }
    }
}

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
    /// `as_slice` given a sample type that matches the inner sample format.
    pub fn len(&self) -> usize {
        self.len
    }

    /// The raw slice of memory representing the underlying audio data as a slice of bytes.
    ///
    /// It is up to the user to interpret the slice of memory based on `Data::sample_format`.
    pub fn bytes(&self) -> &[u8] {
        let len = self.len * self.sample_format.sample_size();
        // The safety of this block relies on correct construction of the `Data` instance. See
        // the unsafe `from_parts` constructor for these requirements.
        unsafe { std::slice::from_raw_parts(self.data as *const u8, len) }
    }

    /// The raw slice of memory representing the underlying audio data as a slice of bytes.
    ///
    /// It is up to the user to interpret the slice of memory based on `Data::sample_format`.
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
        T: Sample,
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
        T: Sample,
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
    pub fn channels(&self) -> ChannelCount {
        self.channels
    }

    pub fn min_sample_rate(&self) -> SampleRate {
        self.min_sample_rate
    }

    pub fn max_sample_rate(&self) -> SampleRate {
        self.max_sample_rate
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    /// Retrieve a `SupportedStreamConfig` with the given sample rate.
    ///
    /// **panic!**s if the given `sample_rate` is outside the range specified within this
    /// `SupportedStreamConfigRange` instance.
    pub fn with_sample_rate(self, sample_rate: SampleRate) -> SupportedStreamConfig {
        assert!(self.min_sample_rate <= sample_rate && sample_rate <= self.max_sample_rate);
        SupportedStreamConfig {
            channels: self.channels,
            sample_format: self.sample_format,
            sample_rate,
        }
    }

    /// Turns this `SupportedStreamConfigRange` into a `SupportedStreamConfig` corresponding to the maximum samples rate.
    #[inline]
    pub fn with_max_sample_rate(self) -> SupportedStreamConfig {
        SupportedStreamConfig {
            channels: self.channels,
            sample_rate: self.max_sample_rate,
            sample_format: self.sample_format,
        }
    }

    /// A comparison function which compares two `SupportedStreamConfigRange`s in terms of their priority of
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

impl From<SupportedStreamConfig> for StreamConfig {
    fn from(conf: SupportedStreamConfig) -> Self {
        conf.config()
    }
}

impl From<SupportedStreamConfig> for SupportedStreamConfigRange {
    #[inline]
    fn from(format: SupportedStreamConfig) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange {
            channels: format.channels,
            min_sample_rate: format.sample_rate,
            max_sample_rate: format.sample_rate,
            sample_format: format.sample_format,
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

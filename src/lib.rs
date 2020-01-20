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
//! use cpal::Data;
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! # let host = cpal::default_host();
//! # let device = host.default_output_device().unwrap();
//! # let format = device.default_output_format().unwrap();
//! let stream = device.build_output_stream(
//!     &format,
//!     move |data: &mut Data| {
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
//! # let format = device.default_output_format().unwrap();
//! let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);
//! let data_fn = move |data: &mut Data| match data.sample_format() {
//!     SampleFormat::F32 => write_silence::<f32>(data),
//!     SampleFormat::I16 => write_silence::<i16>(data),
//!     SampleFormat::U16 => write_silence::<u16>(data),
//! };
//! let stream = device.build_output_stream(&format, data_fn, err_fn).unwrap();
//!
//! fn write_silence<T: Sample>(data: &mut Data) {
//!     let data = data.as_slice_mut::<T>().unwrap();
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
//! # let format = device.default_output_format().unwrap();
//! # let data_fn = move |_data: &mut cpal::Data| {};
//! # let err_fn = move |_err| {};
//! # let stream = device.build_output_stream(&format, data_fn, err_fn).unwrap();
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
//! # let data_fn = move |_data: &mut cpal::Data| {};
//! # let err_fn = move |_err| {};
//! # let stream = device.build_output_stream(&format, data_fn, err_fn).unwrap();
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
    ALL_HOSTS, available_hosts, default_host, Device, Devices, Host, host_from_id,
    HostId, Stream, SupportedInputFormats, SupportedOutputFormats,
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

/// Represents a buffer of audio data, delivered via a user's stream data callback function.
///
/// Input stream callbacks receive `&Data`, while output stream callbacks expect `&mut Data`.
#[derive(Debug)]
pub struct Data {
    data: *mut (),
    len: usize,
    sample_format: SampleFormat,
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
        Data { data, len, sample_format }
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
        unsafe {
            std::slice::from_raw_parts(self.data as *const u8, len)
        }
    }

    /// The raw slice of memory representing the underlying audio data as a slice of bytes.
    ///
    /// It is up to the user to interpret the slice of memory based on `Data::sample_format`.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        let len = self.len * self.sample_format.sample_size();
        // The safety of this block relies on correct construction of the `Data` instance. See
        // the unsafe `from_parts` constructor for these requirements.
        unsafe {
            std::slice::from_raw_parts_mut(self.data as *mut u8, len)
        }
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
            unsafe {
                Some(std::slice::from_raw_parts(self.data as *const T, self.len))
            }
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
                Some(std::slice::from_raw_parts_mut(self.data as *mut T, self.len))
            }
        } else {
            None
        }
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

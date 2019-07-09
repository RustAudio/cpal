//! The suite of traits allowing CPAL to abstract over hosts, devices, event loops and stream IDs.

use {
    BuildStreamError,
    DefaultFormatError,
    DeviceNameError,
    DevicesError,
    Format,
    InputDevices,
    OutputDevices,
    PauseStreamError,
    PlayStreamError,
    StreamDataResult,
    SupportedFormat,
    SupportedFormatsError,
};

/// A **Host** provides access to the available audio devices on the system.
///
/// Each platform may have a number of available hosts depending on the system, each with their own
/// pros and cons.
///
/// For example, WASAPI is the standard audio host API that ships with the Windows operating
/// system. However, due to historical limitations with respect to performance and flexibility,
/// Steinberg created the ASIO API providing better audio device support for pro audio and
/// low-latency applications. As a result, it is common for some devices and device capabilities to
/// only be available via ASIO, while others are only available via WASAPI.
///
/// Another great example is the Linux platform. While the ALSA host API is the lowest-level API
/// available to almost all distributions of Linux, its flexibility is limited as it requires that
/// each process have exclusive access to the devices with which they establish streams. PulseAudio
/// is another popular host API that aims to solve this issue by providing user-space mixing,
/// however it has its own limitations w.r.t. low-latency and high-performance audio applications.
/// JACK is yet another host API that is more suitable to pro-audio applications, however it is
/// less readily available by default in many Linux distributions and is known to be tricky to
/// setup.
pub trait HostTrait {
    /// The type used for enumerating available devices by the host.
    type Devices: Iterator<Item = Self::Device>;
    /// The `Device` type yielded by the host.
    type Device: DeviceTrait;

    /// Whether or not the host is available on the system.
    fn is_available() -> bool;

    /// An iterator yielding all `Device`s currently available to the host on the system.
    ///
    /// Can be empty if the system does not support audio in general.
    fn devices(&self) -> Result<Self::Devices, DevicesError>;

    /// The default input audio device on the system.
    ///
    /// Returns `None` if no input device is available.
    fn default_input_device(&self) -> Option<Self::Device>;

    /// The default output audio device on the system.
    ///
    /// Returns `None` if no output device is available.
    fn default_output_device(&self) -> Option<Self::Device>;

    /// An iterator yielding all `Device`s currently available to the system that support one or more
    /// input stream formats.
    ///
    /// Can be empty if the system does not support audio input.
    fn input_devices(&self) -> Result<InputDevices<Self::Devices>, DevicesError> {
        fn supports_input<D: DeviceTrait>(device: &D) -> bool {
            device.supported_input_formats()
                .map(|mut iter| iter.next().is_some())
                .unwrap_or(false)
        }
        Ok(self.devices()?.filter(supports_input::<Self::Device>))
    }

    /// An iterator yielding all `Device`s currently available to the system that support one or more
    /// output stream formats.
    ///
    /// Can be empty if the system does not support audio output.
    fn output_devices(&self) -> Result<OutputDevices<Self::Devices>, DevicesError> {
        fn supports_output<D: DeviceTrait>(device: &D) -> bool {
            device.supported_output_formats()
                .map(|mut iter| iter.next().is_some())
                .unwrap_or(false)
        }
        Ok(self.devices()?.filter(supports_output::<Self::Device>))
    }
}

/// A device that is capable of audio input and/or output.
///
/// Please note that `Device`s may become invalid if they get disconnected. Therefore all the
/// methods that involve a device return a `Result` allowing the user to handle this case.
pub trait DeviceTrait {
    /// The iterator type yielding supported input stream formats.
    type SupportedInputFormats: Iterator<Item = SupportedFormat>;
    /// The iterator type yielding supported output stream formats.
    type SupportedOutputFormats: Iterator<Item = SupportedFormat>;
    /// The stream type created by `build_input_stream` and `build_output_stream`.
    type Stream: StreamTrait;

    /// The human-readable name of the device.
    fn name(&self) -> Result<String, DeviceNameError>;

    /// An iterator yielding formats that are supported by the backend.
    ///
    /// Can return an error if the device is no longer valid (eg. it has been disconnected).
    fn supported_input_formats(&self) -> Result<Self::SupportedInputFormats, SupportedFormatsError>;

    /// An iterator yielding output stream formats that are supported by the device.
    ///
    /// Can return an error if the device is no longer valid (eg. it has been disconnected).
    fn supported_output_formats(&self) -> Result<Self::SupportedOutputFormats, SupportedFormatsError>;

    /// The default input stream format for the device.
    fn default_input_format(&self) -> Result<Format, DefaultFormatError>;

    /// The default output stream format for the device.
    fn default_output_format(&self) -> Result<Format, DefaultFormatError>;

    /// Create an input stream.
    fn build_input_stream<F>(&self, format: &Format, callback: F) -> Result<Self::Stream, BuildStreamError>
        where F: FnMut(StreamDataResult) + Send + 'static;

    /// Create an output stream.
    fn build_output_stream<F>(&self, format: &Format, callback: F) -> Result<Self::Stream, BuildStreamError>
        where F: FnMut(StreamDataResult) + Send + 'static;
}

/// A stream created from `Device`, with methods to control playback.
pub trait StreamTrait {
    fn play(&self) -> Result<(), PlayStreamError>;

    fn pause(&self) -> Result<(), PauseStreamError>;
}
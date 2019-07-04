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
    /// The event loop type used by the `Host`
    type EventLoop: EventLoopTrait<Device = Self::Device>;

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

    /// Initialise the event loop, ready for managing audio streams.
    fn event_loop(&self) -> Self::EventLoop;

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
}

/// Collection of streams managed together.
///
/// Created with the `Host::event_loop` method.
pub trait EventLoopTrait {
    /// The `Device` type yielded by the host.
    type Device: DeviceTrait;
    /// The type used to uniquely distinguish between streams.
    type StreamId: StreamIdTrait;

    /// Creates a new input stream that will run from the given device and with the given format.
    ///
    /// On success, returns an identifier for the stream.
    ///
    /// Can return an error if the device is no longer valid, or if the input stream format is not
    /// supported by the device.
    fn build_input_stream(
        &self,
        device: &Self::Device,
        format: &Format,
    ) -> Result<Self::StreamId, BuildStreamError>;

    /// Creates a new output stream that will play on the given device and with the given format.
    ///
    /// On success, returns an identifier for the stream.
    ///
    /// Can return an error if the device is no longer valid, or if the output stream format is not
    /// supported by the device.
    fn build_output_stream(
        &self,
        device: &Self::Device,
        format: &Format,
    ) -> Result<Self::StreamId, BuildStreamError>;

    /// Instructs the audio device that it should start playing the stream with the given ID.
    ///
    /// Has no effect is the stream was already playing.
    ///
    /// Only call this after you have submitted some data, otherwise you may hear some glitches.
    ///
    /// # Panic
    ///
    /// If the stream does not exist, this function can either panic or be a no-op.
    fn play_stream(&self, stream: Self::StreamId) -> Result<(), PlayStreamError>;

    /// Instructs the audio device that it should stop playing the stream with the given ID.
    ///
    /// Has no effect is the stream was already paused.
    ///
    /// If you call `play` afterwards, the playback will resume where it was.
    ///
    /// # Panic
    ///
    /// If the stream does not exist, this function can either panic or be a no-op.
    fn pause_stream(&self, stream: Self::StreamId) -> Result<(), PauseStreamError>;

    /// Destroys an existing stream.
    ///
    /// # Panic
    ///
    /// If the stream does not exist, this function can either panic or be a no-op.
    fn destroy_stream(&self, stream: Self::StreamId);

    /// Takes control of the current thread and begins the stream processing.
    ///
    /// > **Note**: Since it takes control of the thread, this method is best called on a separate
    /// > thread.
    ///
    /// Whenever a stream needs to be fed some data, the closure passed as parameter is called.
    /// You can call the other methods of `EventLoop` without getting a deadlock.
    fn run<F>(&self, callback: F) -> !
    where
        F: FnMut(Self::StreamId, StreamDataResult) + Send;
}

/// The set of required bounds for host `StreamId` types.
pub trait StreamIdTrait: Clone + std::fmt::Debug + PartialEq + Eq {}

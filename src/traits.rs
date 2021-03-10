//! The suite of traits allowing CPAL to abstract over hosts, devices, event loops and stream IDs.

use {
    BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, DevicesError,
    DuplexCallbackInfo, DuplexDevices, DuplexStreamConfig, InputCallbackInfo, InputDevices,
    OutputCallbackInfo, OutputDevices, PauseStreamError, PlayStreamError, Sample, SampleFormat,
    StreamConfig, StreamError, SupportedDuplexStreamConfig, SupportedDuplexStreamConfigRange,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
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
/// set up.
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

    /// The default duplex audio device on the system.
    ///
    /// Returns `None` if no duplex device is available.
    fn default_duplex_device(&self) -> Option<Self::Device>;

    /// An iterator yielding all `Device`s currently available to the system that support one or more
    /// input stream formats.
    ///
    /// Can be empty if the system does not support audio input.
    fn input_devices(&self) -> Result<InputDevices<Self::Devices>, DevicesError> {
        fn supports_input<D: DeviceTrait>(device: &D) -> bool {
            device
                .supported_input_configs()
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
            device
                .supported_output_configs()
                .map(|mut iter| iter.next().is_some())
                .unwrap_or(false)
        }
        Ok(self.devices()?.filter(supports_output::<Self::Device>))
    }

    /// An iterator yielding all `Device`s currently available to the system that support one or more
    /// duplex stream formats.
    ///
    /// Can be empty if the system does not support duplex audio.
    fn duplex_devices(&self) -> Result<DuplexDevices<Self::Devices>, DevicesError> {
        fn supports_duplex<D: DeviceTrait>(device: &D) -> bool {
            device
                .supported_duplex_configs()
                .map(|mut iter| iter.next().is_some())
                .unwrap_or(false)
        }
        Ok(self.devices()?.filter(supports_duplex::<Self::Device>))
    }
}

/// A device that is capable of audio input and/or output, or both synchronized as full duplex.
///
/// Please note that `Device`s may become invalid if they get disconnected. Therefore, all the
/// methods that involve a device return a `Result` allowing the user to handle this case.
pub trait DeviceTrait {
    /// The iterator type yielding supported input stream formats.
    type SupportedInputConfigs: Iterator<Item = SupportedStreamConfigRange>;
    /// The iterator type yielding supported output stream formats.
    type SupportedOutputConfigs: Iterator<Item = SupportedStreamConfigRange>;
    /// The iterator type yielding supported duplex stream formats.
    type SupportedDuplexConfigs: Iterator<Item = SupportedDuplexStreamConfigRange>;
    /// The stream type created by `build_input_stream_raw`, `build_output_stream_raw`
    /// and `build_duplex_stream_raw`.
    type Stream: StreamTrait;

    /// The human-readable name of the device.
    fn name(&self) -> Result<String, DeviceNameError>;

    /// An iterator yielding formats that are supported by the backend.
    ///
    /// Can return an error if the device is no longer valid (e.g. it has been disconnected).
    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError>;

    /// An iterator yielding output stream formats that are supported by the device.
    ///
    /// Can return an error if the device is no longer valid (e.g. it has been disconnected).
    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError>;

    /// An iterator yielding duplex stream formats that are supported by the device.
    ///
    /// Can return an error if the device is no longer valid (eg. it has been disconnected).
    fn supported_duplex_configs(
        &self,
    ) -> Result<Self::SupportedDuplexConfigs, SupportedStreamConfigsError>;

    /// The default input stream format for the device.
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError>;

    /// The default output stream format for the device.
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError>;

    /// The default duplex stream format for the device.
    fn default_duplex_config(
        &self,
    ) -> Result<SupportedDuplexStreamConfig, DefaultStreamConfigError>;

    /// Create an input stream.
    fn build_input_stream<T, D, E>(
        &self,
        config: &StreamConfig,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        T: Sample,
        D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.build_input_stream_raw(
            config,
            T::FORMAT,
            move |data, info| {
                data_callback(
                    data.as_slice()
                        .expect("host supplied incorrect sample type"),
                    info,
                )
            },
            error_callback,
        )
    }

    /// Create an output stream.
    fn build_output_stream<T, D, E>(
        &self,
        config: &StreamConfig,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        T: Sample,
        D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.build_output_stream_raw(
            config,
            T::FORMAT,
            move |data, info| {
                data_callback(
                    data.as_slice_mut()
                        .expect("host supplied incorrect sample type"),
                    info,
                )
            },
            error_callback,
        )
    }

    /// Create a duplex stream.
    fn build_duplex_stream<T, D, E>(
        &self,
        config: &DuplexStreamConfig,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        T: Sample,
        D: FnMut(&[T], &mut [T], &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.build_duplex_stream_raw(
            config,
            T::FORMAT,
            move |input_data, output_data, info| {
                data_callback(
                    input_data
                        .as_slice_mut()
                        .expect("host supplied incorrect sample type"),
                    output_data
                        .as_slice_mut()
                        .expect("host supplied incorrect sample type"),
                    info,
                )
            },
            error_callback,
        )
    }

    /// Create a dynamically typed input stream.
    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static;

    /// Create a dynamically typed output stream.
    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static;

    /// Create a dynamically typed duplex stream.
    fn build_duplex_stream_raw<D, E>(
        &self,
        config: &DuplexStreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static;
}

/// A stream created from `Device`, with methods to control playback.
pub trait StreamTrait {
    /// Run the stream.
    ///
    /// Note: Not all platforms automatically run the stream upon creation, so it is important to
    /// call `play` after creation if it is expected that the stream should run immediately.
    fn play(&self) -> Result<(), PlayStreamError>;

    /// Some devices support pausing the audio stream. This can be useful for saving energy in
    /// moments of silence.
    ///
    /// Note: Not all devices support suspending the stream at the hardware level. This method may
    /// fail in these cases.
    fn pause(&self) -> Result<(), PauseStreamError>;
}

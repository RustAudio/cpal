//! The suite of traits allowing CPAL to abstract over hosts, devices, event loops and stream IDs.
//!
//! # Custom Host Implementations
//!
//! When implementing custom hosts with the `custom` feature, your `Device` type must implement
//! [`DeviceTrait`] and your `Stream` type must implement [`StreamTrait`].

use std::{
    fmt::{Debug, Display},
    hash::Hash,
    time::Duration,
};

use crate::{
    CallbackInfo, Data, DeviceDescription, DeviceId, DuplexCallbackInfo, DuplexStreamConfig, Error,
    ErrorKind, InputDevices, OutputDevices, SampleFormat, SizedSample, StreamConfig, StreamInstant,
    SupportedStreamConfig, SupportedStreamConfigRange,
};

/// A [`Host`] provides access to the available audio devices on the system.
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
///
/// [`Host`]: crate::Host
pub trait HostTrait {
    /// The type used for enumerating available devices by the host.
    type Devices: Iterator<Item = Self::Device>;
    /// The `Device` type yielded by the host.
    type Device: DeviceTrait;

    /// Whether or not the host is available on the system.
    fn is_available() -> bool;

    /// An iterator yielding all [`Device`](DeviceTrait)s currently available to the host on the system.
    ///
    /// Can be empty if the system does not support audio in general.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::HostUnavailable`] if the host has become unreachable (e.g. the audio
    ///   daemon crashed or was stopped).
    /// - [`ErrorKind::BackendError`] for unclassifiable backend failures.
    ///
    /// [`ErrorKind::HostUnavailable`]: crate::ErrorKind::HostUnavailable
    /// [`ErrorKind::BackendError`]: crate::ErrorKind::BackendError
    fn devices(&self) -> Result<Self::Devices, Error>;

    /// Fetches a [`Device`](DeviceTrait) based on a [`DeviceId`] if available
    ///
    /// Returns `None` if no device matching the id is found
    fn device_by_id(&self, id: &DeviceId) -> Option<Self::Device> {
        self.devices()
            .ok()?
            .find(|device| device.id().ok().as_ref() == Some(id))
    }

    /// The default input audio device on the system.
    ///
    /// Returns `None` if no input device is available.
    ///
    /// Some backends reroute a stream built from this device to the new default device when it
    /// changes; capture continues there, and [`ErrorKind::DeviceChanged`] is reported. Other
    /// backends report [`ErrorKind::DeviceNotAvailable`] instead, and the caller must rebuild the
    /// stream.
    ///
    /// [`ErrorKind::DeviceChanged`]: crate::ErrorKind::DeviceChanged
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    fn default_input_device(&self) -> Option<Self::Device>;

    /// The default output audio device on the system.
    ///
    /// Returns `None` if no output device is available.
    ///
    /// Some backends reroute a stream built from this device to the new default device when it
    /// changes; playback continues there, and [`ErrorKind::DeviceChanged`] is reported. Other
    /// backends report [`ErrorKind::DeviceNotAvailable`] instead, and the caller must rebuild the
    /// stream.
    ///
    /// [`ErrorKind::DeviceChanged`]: crate::ErrorKind::DeviceChanged
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    fn default_output_device(&self) -> Option<Self::Device>;

    /// An iterator yielding all `Device`s currently available to the system that support one or more
    /// input stream formats.
    ///
    /// Can be empty if the system does not support audio input.
    ///
    /// # Errors
    ///
    /// Propagates errors from [`devices`](Self::devices).
    fn input_devices(&self) -> Result<InputDevices<Self::Devices>, Error> {
        Ok(self.devices()?.filter(DeviceTrait::supports_input))
    }

    /// An iterator yielding all `Device`s currently available to the system that support one or more
    /// output stream formats.
    ///
    /// Can be empty if the system does not support audio output.
    ///
    /// # Errors
    ///
    /// Propagates errors from [`devices`](Self::devices).
    fn output_devices(&self) -> Result<OutputDevices<Self::Devices>, Error> {
        Ok(self.devices()?.filter(DeviceTrait::supports_output))
    }
}

/// A device that is capable of audio input and/or output.
///
/// Please note that `Device`s may become invalid if they get disconnected. Therefore, all the
/// methods that involve a device return a `Result` allowing the user to handle this case.
pub trait DeviceTrait: PartialEq + Eq + Hash + Debug + Display + Send + Sync {
    /// The iterator type yielding supported input stream formats.
    type SupportedInputConfigs: Iterator<Item = SupportedStreamConfigRange>;
    /// The iterator type yielding supported output stream formats.
    type SupportedOutputConfigs: Iterator<Item = SupportedStreamConfigRange>;
    /// The stream type created by [`build_input_stream_raw`] and [`build_output_stream_raw`],
    /// and [`build_duplex_stream_raw`].
    ///
    /// [`build_input_stream_raw`]: Self::build_input_stream_raw
    /// [`build_output_stream_raw`]: Self::build_output_stream_raw
    /// [`build_duplex_stream_raw`]: Self::build_duplex_stream_raw
    type Stream: StreamTrait;

    /// Structured description of the device with metadata.
    ///
    /// This returns a [`DeviceDescription`] containing structured information about the device,
    /// including name, manufacturer (if available), device type, bus type, and other
    /// platform-specific metadata.
    ///
    /// For the device name as a string, use `device.to_string()` or format it with `{}`. For the
    /// full structured description, call `device.description()?` and format or inspect that.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    fn description(&self) -> Result<DeviceDescription, Error>;

    /// The ID of the device.
    ///
    /// This ID uniquely identifies the device on the host. It should be stable across program
    /// runs, device disconnections, and system reboots where possible.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    fn id(&self) -> Result<DeviceId, Error>;

    /// True if the device supports audio input, otherwise false
    fn supports_input(&self) -> bool {
        self.supported_input_configs()
            .is_ok_and(|mut iter| iter.next().is_some())
    }

    /// True if the device supports audio output, otherwise false
    fn supports_output(&self) -> bool {
        self.supported_output_configs()
            .is_ok_and(|mut iter| iter.next().is_some())
    }

    /// True if the device can build a synchronized duplex stream where the captured input and
    /// rendered output share a single clock.
    ///
    /// Returning `true` is a contract that input and output sides will run from one device-level
    /// callback, or an OS driver aggregate (such as an Aggregate Device on macOS).
    ///
    /// The default implementation returns `false`; hosts that can guarantee a shared clock should
    /// override.
    fn supports_duplex(&self) -> bool {
        false
    }

    /// An iterator yielding input stream configurations that are supported by the device.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::UnsupportedOperation`] if the device does not support input.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error>;

    /// An iterator yielding output stream configurations that are supported by the device.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::UnsupportedOperation`] if the device does not support output.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error>;

    /// The default input stream configuration for the device.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::UnsupportedConfig`] if the device has no default input configuration.
    /// - [`ErrorKind::UnsupportedOperation`] if the device does not support input.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::UnsupportedConfig`]: crate::ErrorKind::UnsupportedConfig
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error>;

    /// The default output stream configuration for the device.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::UnsupportedConfig`] if the device has no default output configuration.
    /// - [`ErrorKind::UnsupportedOperation`] if the device does not support output.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::UnsupportedConfig`]: crate::ErrorKind::UnsupportedConfig
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error>;

    /// Create an input stream.
    ///
    /// # Parameters
    ///
    /// * `config` - The stream configuration including sample rate, channels, and buffer size.
    /// * `data_callback` - Called periodically with captured audio data. The callback receives
    ///   a slice of samples in the format `T` and timing information.
    /// * `error_callback` - Called when a stream error occurs (e.g., device disconnected).
    /// * `timeout` - Time to wait for the backend to initialize the stream. `None` waits
    ///   indefinitely; `Some(duration)` limits how long to wait. Note: not all backends honor
    ///   this value.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedConfig`] if the sample rate, channel count, buffer size, or
    ///   sample format is not supported by the device.
    /// - [`ErrorKind::UnsupportedOperation`] if the device does not support input streams.
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::DeviceBusy`] if the device is temporarily in use by another application.
    /// - [`ErrorKind::PermissionDenied`] if the process lacks permission to access the device.
    /// - [`ErrorKind::InvalidInput`] if the configuration parameters are invalid.
    ///
    /// [`ErrorKind::UnsupportedConfig`]: crate::ErrorKind::UnsupportedConfig
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::DeviceBusy`]: crate::ErrorKind::DeviceBusy
    /// [`ErrorKind::PermissionDenied`]: crate::ErrorKind::PermissionDenied
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    fn build_input_stream<T, D, E>(
        &self,
        config: StreamConfig,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        T: SizedSample,
        D: FnMut(&[T], &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
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
            timeout,
        )
    }

    /// Create an output stream.
    ///
    /// # Parameters
    ///
    /// * `config` - The stream configuration including sample rate, channels, and buffer size.
    /// * `data_callback` - Called periodically to fill the output buffer. The callback receives
    ///   a mutable slice of samples in the format `T` to be filled with audio data, along with
    ///   timing information. The slice is pre-filled with silence, so a callback that writes
    ///   fewer samples than the slice holds leaves the remainder silent rather than stale.
    /// * `error_callback` - Called when a stream error occurs (e.g., device disconnected).
    /// * `timeout` - Time to wait for the backend to initialize the stream. `None` waits
    ///   indefinitely; `Some(duration)` limits how long to wait. Note: not all backends honor
    ///   this value.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedConfig`] if the sample rate, channel count, buffer size, or
    ///   sample format is not supported by the device.
    /// - [`ErrorKind::UnsupportedOperation`] if the device does not support output streams.
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::DeviceBusy`] if the device is temporarily in use by another application.
    /// - [`ErrorKind::PermissionDenied`] if the process lacks permission to access the device.
    /// - [`ErrorKind::InvalidInput`] if the configuration parameters are invalid.
    ///
    /// [`ErrorKind::UnsupportedConfig`]: crate::ErrorKind::UnsupportedConfig
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::DeviceBusy`]: crate::ErrorKind::DeviceBusy
    /// [`ErrorKind::PermissionDenied`]: crate::ErrorKind::PermissionDenied
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    fn build_output_stream<T, D, E>(
        &self,
        config: StreamConfig,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        T: SizedSample,
        D: FnMut(&mut [T], &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
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
            timeout,
        )
    }

    /// Create a dynamically typed input stream.
    ///
    /// This method allows working with sample data as raw bytes, useful when the sample
    /// format is determined at runtime. For compile-time known formats, prefer
    /// [`build_input_stream`](Self::build_input_stream).
    ///
    /// # Parameters
    ///
    /// * `config` - The stream configuration including sample rate, channels, and buffer size.
    /// * `sample_format` - The sample format of the audio data.
    /// * `data_callback` - Called periodically with captured audio data as a [`Data`] buffer.
    /// * `error_callback` - Called when a stream error occurs (e.g., device disconnected).
    /// * `timeout` - Time to wait for the backend to initialize the stream. `None` waits
    ///   indefinitely; `Some(duration)` limits how long to wait. Note: not all backends honor
    ///   this value.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedConfig`] if the sample rate, channel count, buffer size, or
    ///   sample format is not supported by the device.
    /// - [`ErrorKind::UnsupportedOperation`] if the device does not support input streams.
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::DeviceBusy`] if the device is temporarily in use by another application.
    /// - [`ErrorKind::PermissionDenied`] if the process lacks permission to access the device.
    /// - [`ErrorKind::InvalidInput`] if the configuration parameters are invalid.
    ///
    /// [`ErrorKind::UnsupportedConfig`]: crate::ErrorKind::UnsupportedConfig
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::DeviceBusy`]: crate::ErrorKind::DeviceBusy
    /// [`ErrorKind::PermissionDenied`]: crate::ErrorKind::PermissionDenied
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static;

    /// Create a dynamically typed output stream.
    ///
    /// This method allows working with sample data as raw bytes, useful when the sample
    /// format is determined at runtime. For compile-time known formats, prefer
    /// [`build_output_stream`](Self::build_output_stream).
    ///
    /// # Parameters
    ///
    /// * `config` - The stream configuration including sample rate, channels, and buffer size.
    /// * `sample_format` - The sample format of the audio data.
    /// * `data_callback` - Called periodically to fill the output buffer with audio data as
    ///   a mutable [`Data`] buffer. The buffer is pre-filled with silence, so a callback that
    ///   writes fewer samples than the buffer holds leaves the remainder silent rather than stale.
    /// * `error_callback` - Called when a stream error occurs (e.g., device disconnected).
    /// * `timeout` - Time to wait for the backend to initialize the stream. `None` waits
    ///   indefinitely; `Some(duration)` limits how long to wait. Note: not all backends honor
    ///   this value.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedConfig`] if the sample rate, channel count, buffer size, or
    ///   sample format is not supported by the device.
    /// - [`ErrorKind::UnsupportedOperation`] if the device does not support output streams.
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::DeviceBusy`] if the device is temporarily in use by another application.
    /// - [`ErrorKind::PermissionDenied`] if the process lacks permission to access the device.
    /// - [`ErrorKind::InvalidInput`] if the configuration parameters are invalid.
    ///
    /// [`ErrorKind::UnsupportedConfig`]: crate::ErrorKind::UnsupportedConfig
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::DeviceBusy`]: crate::ErrorKind::DeviceBusy
    /// [`ErrorKind::PermissionDenied`]: crate::ErrorKind::PermissionDenied
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static;

    /// Create a synchronized duplex stream whose input and output share the same clock
    /// or OS provided bidirectional aggregate device (macOS). macOS Aggregate device drift
    /// compensation is not required.
    ///
    /// # Parameters
    ///
    /// * `config` - Channels, sample rate, and buffer size shared by both directions.
    /// * `data_callback` - Called periodically with captured input and a mutable output buffer.
    /// * `error_callback` - Called when a stream error occurs (e.g., device disconnected).
    /// * `timeout` - Time to wait for the backend to initialize the stream. `None` waits
    ///   indefinitely; `Some(duration)` limits how long to wait. Note: not all backends honor
    ///   this value.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedOperation`] if the host or device does not support duplex
    ///   streams.
    /// - [`ErrorKind::UnsupportedConfig`] if the sample rate, channel counts, buffer size, or
    ///   sample format is not supported by the device.
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::DeviceBusy`] if the device is temporarily in use by another application.
    /// - [`ErrorKind::PermissionDenied`] if the process lacks permission to access the device
    ///   (e.g. microphone access on macOS).
    /// - [`ErrorKind::InvalidInput`] if the configuration parameters are invalid.
    /// - [`ErrorKind::StreamInvalidated`] if the device's sample rate or buffer size changed
    ///   during stream creation, or an internal lock was poisoned.
    /// - [`ErrorKind::ResourceExhausted`] if the host fails to spawn an internal monitoring
    ///   thread.
    /// - [`ErrorKind::BackendError`] for unclassified backend failures.
    ///
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::UnsupportedConfig`]: crate::ErrorKind::UnsupportedConfig
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::DeviceBusy`]: crate::ErrorKind::DeviceBusy
    /// [`ErrorKind::PermissionDenied`]: crate::ErrorKind::PermissionDenied
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    /// [`ErrorKind::StreamInvalidated`]: crate::ErrorKind::StreamInvalidated
    /// [`ErrorKind::ResourceExhausted`]: crate::ErrorKind::ResourceExhausted
    /// [`ErrorKind::BackendError`]: crate::ErrorKind::BackendError
    fn build_duplex_stream<I, O, D, E>(
        &self,
        config: DuplexStreamConfig,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        I: SizedSample,
        O: SizedSample,
        D: FnMut(&[I], &mut [O], &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        self.build_duplex_stream_raw(
            config,
            I::FORMAT,
            O::FORMAT,
            move |input, output, info| {
                data_callback(
                    input
                        .as_slice()
                        .expect("host supplied incorrect sample type"),
                    output
                        .as_slice_mut()
                        .expect("host supplied incorrect sample type"),
                    info,
                )
            },
            error_callback,
            timeout,
        )
    }

    /// Create a dynamically typed synchronized duplex stream.
    ///
    /// Hosts that support duplex streams must override this method;
    /// the default implementation returns [`ErrorKind::UnsupportedOperation`].
    ///
    /// See [`build_duplex_stream`](Self::build_duplex_stream) for parameter and error
    /// documentation.
    ///
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    fn build_duplex_stream_raw<D, E>(
        &self,
        _config: DuplexStreamConfig,
        _input_sample_format: SampleFormat,
        _output_sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        Err(Error::with_message(
            ErrorKind::UnsupportedOperation,
            "duplex streams are not supported by this device",
        ))
    }

    /// Obtain the associated string name for a channel index.
    ///
    /// This method is only implemented for CoreAudio (macOS) and ASIO (Windows). All other
    /// backends will return [`ErrorKind::UnsupportedOperation`].
    ///
    /// # Parameters
    ///
    /// * `channel_index` - Channel index to query name for.
    /// * `input` - Whether to query an input channel (true) or output channel (false).
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedOperation`] if the backend does not implement channel name
    ///   queries.
    /// - [`ErrorKind::InvalidInput`] if the channel index is out of range for the device,
    ///   or if the device does not support the requested direction (input/output).
    /// - [`ErrorKind::Other`] for unclassifiable backend failures (e.g., the channel name could
    ///   not be retrieved from the device).
    ///
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    /// [`ErrorKind::Other`]: crate::ErrorKind::Other
    fn get_channel_name(&self, _channel_index: u16, _input: bool) -> Result<String, Error> {
        Err(Error::with_message(
            ErrorKind::UnsupportedOperation,
            "device does not support channel names",
        ))
    }
}

/// A stream created from [`Device`](DeviceTrait), with methods to control it.
pub trait StreamTrait: Send + Sync {
    /// Start (or resume) the stream.
    ///
    /// Streams returned by `build_*_stream` are always stopped, so `start` must be called before
    /// the data callback will fire.
    ///
    /// `start` also resumes a stream that was halted with [`pause`](Self::pause) or
    /// [`stop`](Self::stop): after a [`stop`](Self::stop) the backend re-prepares the device as
    /// needed, so the same stream can be cycled through start/stop without being rebuilt.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::StreamInvalidated`] if the stream configuration has changed and the stream
    ///   must be rebuilt.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::StreamInvalidated`]: crate::ErrorKind::StreamInvalidated
    fn start(&self) -> Result<(), Error>;

    /// Deprecated alias for [`start`](Self::start).
    #[deprecated(since = "0.19.0", note = "renamed to `start`")]
    fn play(&self) -> Result<(), Error> {
        self.start()
    }

    /// Pause the stream, halting the data callback as soon as possible.
    ///
    /// Pausing halts audio immediately without waiting for buffered output to finish. On backends
    /// that support hardware-level suspend, frames already queued in the device are preserved and
    /// resume playing after the next [`start`](Self::start); on other backends the buffer may be
    /// flushed. To let buffered audio play out before halting, use [`stop`](Self::stop) instead. A
    /// paused stream is resumed with [`start`](Self::start).
    ///
    /// Some devices support suspending at the hardware level (saving energy); others stop only the
    /// data callback while the hardware keeps running.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedOperation`] if the backend does not support pausing this stream.
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::StreamInvalidated`] if the stream configuration has changed and the stream
    ///   must be rebuilt.
    ///
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::StreamInvalidated`]: crate::ErrorKind::StreamInvalidated
    fn pause(&self) -> Result<(), Error>;

    /// Stop the stream gracefully, draining buffered audio before halting.
    ///
    /// Unlike [`pause`](Self::pause), `stop` lets audio that has already been buffered finish:
    /// on an output stream it blocks the calling thread until the device has played out its queued
    /// frames (or `timeout` elapses), then halts.
    ///
    /// The stream remains valid after `stop`; calling [`start`](Self::start) again resumes it,
    /// re-preparing the device if the backend requires it.
    ///
    /// # Parameters
    ///
    /// * `timeout` - How long to wait for buffered audio to drain. `None` waits until the drain
    ///   completes; `Some(duration)` halts after the duration even if frames remain;
    ///   `Some(Duration::ZERO)` halts immediately. Independent of the timeout passed to
    ///   `build_*_stream`.
    ///
    /// # Backend support
    ///
    /// Draining is only meaningful for output streams. Backends may drain natively or approximate
    /// it by sleeping for an estimated buffer depth; if a backend has no drain support at all,
    /// `stop` halts immediately like `pause`. On input (capture) streams, `stop` always halts
    /// immediately and `timeout` is ignored.
    ///
    /// Dropping a stream without calling `stop` halts it immediately, without draining.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::StreamInvalidated`] if the stream configuration has changed and the stream
    ///   must be rebuilt.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::StreamInvalidated`]: crate::ErrorKind::StreamInvalidated
    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error>;

    /// Returns the backend's best available estimate of the number of frames per callback.
    ///
    /// The value is available immediately after stream creation: for fixed buffer sizes this is
    /// the negotiated hardware size; for default buffer sizes this is the backend's configured
    /// default. The value is updated when it changes during the lifetime of the stream.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedOperation`] if the backend cannot query the buffer size.
    /// - [`ErrorKind::BackendError`] for unclassifiable backend failures.
    ///
    /// # Implementation notes
    ///
    /// It is not enforced that each callback delivers exactly this many frames. The actual frame
    /// count for each callback is given by its buffer.
    ///
    /// `buffer_size()` is primarily intended for sizing pre-allocated buffers, but must not be
    /// trusted as a guaranteed bound. An incorrect implementation of `buffer_size()` should not
    /// lead to memory safety violations.
    ///
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::BackendError`]: crate::ErrorKind::BackendError
    fn buffer_size(&self) -> Result<crate::FrameCount, Error>;

    /// Returns a [`StreamInstant`] representing the current moment on the stream's clock.
    ///
    /// The clock is **monotonic**: successive calls to `now()` will never return a value earlier
    /// than a previous one, and the returned value will never be earlier than any `callback`,
    /// `capture`, or `playback` instant already delivered to the stream's data callback.
    ///
    /// The returned value shares the same time base as the [`StreamInstant`]s delivered to the
    /// stream's data callback via [`crate::StreamTimestamp::callback`] and
    /// [`crate::StreamTimestamp::callback`], so durations between them are meaningful.
    fn now(&self) -> StreamInstant;
}

/// Compile-time assertion that a stream type implements [`Send`].
#[deprecated(since = "0.19.0", note = "StreamTrait now requires Send + Sync")]
#[macro_export]
macro_rules! assert_stream_send {
    ($t:ty) => {
        const fn _assert_stream_send<T: Send>() {}
        const _: () = _assert_stream_send::<$t>();
    };
}

/// Compile-time assertion that a stream type implements [`Sync`].
#[deprecated(since = "0.19.0", note = "StreamTrait now requires Send + Sync")]
#[macro_export]
macro_rules! assert_stream_sync {
    ($t:ty) => {
        const fn _assert_stream_sync<T: Sync>() {}
        const _: () = _assert_stream_sync::<$t>();
    };
}

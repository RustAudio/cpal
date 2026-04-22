//! The suite of traits allowing CPAL to abstract over hosts, devices, event loops and stream IDs.
//!
//! # Custom Host Implementations
//!
//! When implementing custom hosts with the `custom` feature, use the [`assert_stream_send!`](crate::assert_stream_send)
//! and [`assert_stream_sync!`](crate::assert_stream_sync) macros to verify your `Stream` type meets CPAL's requirements.

use std::time::Duration;

use crate::{
    Data, DeviceDescription, DeviceId, Error, InputCallbackInfo, InputDevices, OutputCallbackInfo,
    OutputDevices, SampleFormat, SizedSample, StreamConfig, StreamInstant, SupportedStreamConfig,
    SupportedStreamConfigRange,
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
    /// - [`ErrorKind::Other`] for unclassifiable backend failures.
    ///
    /// [`ErrorKind::HostUnavailable`]: crate::ErrorKind::HostUnavailable
    /// [`ErrorKind::Other`]: crate::ErrorKind::Other
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
    fn default_input_device(&self) -> Option<Self::Device>;

    /// The default output audio device on the system.
    ///
    /// Returns `None` if no output device is available.
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
pub trait DeviceTrait {
    /// The iterator type yielding supported input stream formats.
    type SupportedInputConfigs: Iterator<Item = SupportedStreamConfigRange>;
    /// The iterator type yielding supported output stream formats.
    type SupportedOutputConfigs: Iterator<Item = SupportedStreamConfigRange>;
    /// The stream type created by [`build_input_stream_raw`] and [`build_output_stream_raw`].
    ///
    /// [`build_input_stream_raw`]: Self::build_input_stream_raw
    /// [`build_output_stream_raw`]: Self::build_output_stream_raw
    type Stream: StreamTrait;

    /// The human-readable name of the device.
    #[deprecated(
        since = "0.17.0",
        note = "Use `description()` for comprehensive device information including name, \
                manufacturer, and device type. Use `id()` for a unique, stable device identifier \
                that persists across reboots and reconnections."
    )]
    fn name(&self) -> Result<String, Error> {
        self.description().map(|desc| desc.name().to_string())
    }

    /// Structured description of the device with metadata.
    ///
    /// This returns a [`DeviceDescription`] containing structured information about the device,
    /// including name, manufacturer (if available), device type, bus type, and other
    /// platform-specific metadata.
    ///
    /// For simple string representation, use `device.description().to_string()` or
    /// `device.description().name()`.
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
        D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
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
    ///   timing information.
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
        D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
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
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
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
    ///   a mutable [`Data`] buffer.
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
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static;
}

/// A stream created from [`Device`](DeviceTrait), with methods to control playback.
pub trait StreamTrait {
    /// Run the stream.
    ///
    /// Note: Not all platforms automatically run the stream upon creation, so it is important to
    /// call `play` after creation if it is expected that the stream should run immediately.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::StreamInvalidated`] if the stream configuration has changed and the stream
    ///   must be rebuilt.
    ///
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::StreamInvalidated`]: crate::ErrorKind::StreamInvalidated
    fn play(&self) -> Result<(), Error>;

    /// Some devices support pausing the audio stream. This can be useful for saving energy in
    /// moments of silence.
    ///
    /// Note: Not all devices support suspending the stream at the hardware level. This method may
    /// fail in these cases.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedOperation`] if the backend does not support pausing streams.
    /// - [`ErrorKind::DeviceNotAvailable`] if the device has been disconnected.
    /// - [`ErrorKind::StreamInvalidated`] if the stream configuration has changed and the stream
    ///   must be rebuilt.
    ///
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    /// [`ErrorKind::DeviceNotAvailable`]: crate::ErrorKind::DeviceNotAvailable
    /// [`ErrorKind::StreamInvalidated`]: crate::ErrorKind::StreamInvalidated
    fn pause(&self) -> Result<(), Error>;

    /// Returns the backend's best available estimate of the number of frames per callback.
    ///
    /// The value is available immediately after stream creation: for fixed buffer sizes this is
    /// the negotiated hardware size; for default buffer sizes this is the backend's configured
    /// default. The value is updated when it changes during the lifetime of the stream.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedOperation`] if the backend cannot query the buffer size.
    /// - [`ErrorKind::Other`] for unclassifiable backend failures.
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
    /// [`ErrorKind::Other`]: crate::ErrorKind::Other
    fn buffer_size(&self) -> Result<crate::FrameCount, Error>;

    /// Returns a [`StreamInstant`] representing the current moment on the stream's clock.
    ///
    /// The clock is **monotonic**: successive calls to `now()` will never return a value earlier
    /// than a previous one, and the returned value will never be earlier than any `callback`,
    /// `capture`, or `playback` instant already delivered to the stream's data callback.
    ///
    /// The returned value shares the same time base as the [`StreamInstant`]s delivered to the
    /// stream's data callback via [`crate::InputStreamTimestamp::callback`] and
    /// [`crate::OutputStreamTimestamp::callback`], so durations between them are meaningful.
    fn now(&self) -> StreamInstant;
}

/// Compile-time assertion that a stream type implements [`Send`].
///
/// Custom host implementations should use this macro to verify their `Stream` type
/// can be safely transferred between threads, as required by CPAL's API.
///
/// # Example
///
/// ```
/// use cpal::assert_stream_send;
/// struct MyStream { /* ... */ }
/// assert_stream_send!(MyStream);
/// ```
#[macro_export]
macro_rules! assert_stream_send {
    ($t:ty) => {
        const fn _assert_stream_send<T: Send>() {}
        const _: () = _assert_stream_send::<$t>();
    };
}

/// Compile-time assertion that a stream type implements [`Sync`].
///
/// Custom host implementations should use this macro to verify their `Stream` type
/// can be safely shared between threads, as required by CPAL's API.
///
/// # Example
///
/// ```
/// use cpal::assert_stream_sync;
/// struct MyStream { /* ... */ }
/// assert_stream_sync!(MyStream);
/// ```
#[macro_export]
macro_rules! assert_stream_sync {
    ($t:ty) => {
        const fn _assert_stream_sync<T: Sync>() {}
        const _: () = _assert_stream_sync::<$t>();
    };
}

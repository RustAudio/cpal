use std::{
    fmt,
    hash::{Hash, Hasher},
    time::Duration,
};

use super::{JACK_SAMPLE_FORMAT, stream::Stream};
pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};
use crate::{
    BufferSize, CallbackInfo, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder,
    DeviceDirection, DeviceId, DuplexCallbackInfo, DuplexStreamConfig, Error, ErrorKind,
    SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange, traits::DeviceTrait,
};

const DEFAULT_NUM_CHANNELS: ChannelCount = 2;

#[derive(Clone, Debug)]
pub struct Device {
    name: String,
    sample_rate: SampleRate,
    buffer_size: SupportedBufferSize,
    max_input_channels: ChannelCount,
    max_output_channels: ChannelCount,
    direction: DeviceDirection,
    start_server_automatically: bool,
    connect_ports_automatically: bool,
}

impl Device {
    fn new_device(
        name: String,
        connect_ports_automatically: bool,
        start_server_automatically: bool,
        direction: DeviceDirection,
    ) -> Result<Self, Error> {
        let client_options = super::get_client_options(start_server_automatically);

        // Create a dummy client to find out the sample rate of the server to be able to provide it
        // as a possible config. This client will be dropped, and a new one will be created when
        // making the stream. This is a hack due to the fact that the Client must be moved to
        // create the AsyncClient.
        let client = super::get_client(&name, client_options)?;

        // Enumeration reflects the routed system ports; build_*_stream_raw allows more
        // channels than this for patching to an unrouted or downstream JACK client.
        let port_count = |pattern| -> ChannelCount {
            client
                .ports(Some(pattern), None, jack::PortFlags::empty())
                .len()
                .try_into()
                .unwrap_or(DEFAULT_NUM_CHANNELS)
                .max(DEFAULT_NUM_CHANNELS)
        };
        let (max_input_channels, max_output_channels) = match direction {
            DeviceDirection::Input => (port_count("system:capture_.*"), 0),
            DeviceDirection::Output => (0, port_count("system:playback_.*")),
            DeviceDirection::Duplex => (
                port_count("system:capture_.*"),
                port_count("system:playback_.*"),
            ),
            _ => {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedOperation,
                    format!("JACK does not support {direction:?} direction"),
                ));
            }
        };
        Ok(Self {
            // The name given to the client by JACK, could potentially be different from the name
            // supplied e.g. if there is a name collision
            name: client.name().to_owned(),
            sample_rate: client.sample_rate(),
            buffer_size: SupportedBufferSize::Range {
                min: client.buffer_size(),
                max: client.buffer_size(),
            },
            max_input_channels,
            max_output_channels,
            direction,
            start_server_automatically,
            connect_ports_automatically,
        })
    }

    fn id(&self) -> Result<DeviceId, Error> {
        // `self.name` carries the process ID (see `Host::new`) so that concurrent cpal
        // instances get distinct JACK client names. It must not leak into `DeviceId`,
        // which callers persist across restarts: a synthetic device's direction is the
        // only part of its identity that's actually stable.
        let id = match self.direction {
            DeviceDirection::Input => "input",
            DeviceDirection::Output => "output",
            DeviceDirection::Duplex => "duplex",
            _ => "unknown",
        };
        Ok(DeviceId::new(crate::platform::HostId::Jack, id))
    }

    pub fn default_output_device(
        name: &str,
        connect_ports_automatically: bool,
        start_server_automatically: bool,
    ) -> Result<Self, Error> {
        let output_client_name = format!("{}_out", name);
        Device::new_device(
            output_client_name,
            connect_ports_automatically,
            start_server_automatically,
            DeviceDirection::Output,
        )
    }

    pub fn default_input_device(
        name: &str,
        connect_ports_automatically: bool,
        start_server_automatically: bool,
    ) -> Result<Self, Error> {
        let input_client_name = format!("{}_in", name);
        Device::new_device(
            input_client_name,
            connect_ports_automatically,
            start_server_automatically,
            DeviceDirection::Input,
        )
    }

    /// A single JACK client with both input and output ports, delivered together from one
    /// `process()` callback per cycle. Uses the plain client name, with no `_in`/`_out` suffix.
    pub fn default_duplex_device(
        name: &str,
        connect_ports_automatically: bool,
        start_server_automatically: bool,
    ) -> Result<Self, Error> {
        Device::new_device(
            name.to_owned(),
            connect_ports_automatically,
            start_server_automatically,
            DeviceDirection::Duplex,
        )
    }

    pub fn default_config(&self) -> Result<SupportedStreamConfig, Error> {
        let channels = DEFAULT_NUM_CHANNELS;
        let sample_rate = self.sample_rate;
        let buffer_size = self.buffer_size;
        // The sample format for JACK audio ports is always "32-bit float mono audio" in the current implementation.
        // Custom formats are allowed within JACK, but this is of niche interest.
        // The format can be found programmatically by calling jack::PortSpec::port_type() on a created port.
        let sample_format = JACK_SAMPLE_FORMAT;
        Ok(SupportedStreamConfig {
            channels,
            sample_rate,
            buffer_size,
            sample_format,
        })
    }

    fn supported_configs(&self, direction: DeviceDirection) -> Vec<SupportedStreamConfigRange> {
        let max_channels = match direction {
            DeviceDirection::Input => self.max_input_channels,
            DeviceDirection::Output => self.max_output_channels,
            _ => 0,
        };
        let f = match self.default_config() {
            Err(_) => return vec![],
            Ok(f) => f,
        };

        (1..=max_channels)
            .map(|channels| SupportedStreamConfigRange {
                channels,
                min_sample_rate: f.sample_rate,
                max_sample_rate: f.sample_rate,
                buffer_size: f.buffer_size,
                sample_format: f.sample_format,
            })
            .collect()
    }

    pub fn is_input(&self) -> bool {
        matches!(self.direction, DeviceDirection::Input)
    }

    pub fn is_output(&self) -> bool {
        matches!(self.direction, DeviceDirection::Output)
    }

    pub fn is_duplex(&self) -> bool {
        matches!(self.direction, DeviceDirection::Duplex)
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        // Not `self.name`: that's the JACK client name, which carries a process ID
        // uniquifier (see `Host::new`) and isn't meant as a user-facing device label.
        let name = match self.direction {
            DeviceDirection::Input => "JACK Input",
            DeviceDirection::Output => "JACK Output",
            DeviceDirection::Duplex => "JACK Duplex",
            _ => "JACK Device",
        };
        Ok(DeviceDescriptionBuilder::new(name)
            .direction(self.direction)
            .build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Device::id(self)
    }

    fn supports_duplex(&self) -> bool {
        self.is_duplex()
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Ok(self.supported_configs(DeviceDirection::Input).into_iter())
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        Ok(self.supported_configs(DeviceDirection::Output).into_iter())
    }

    /// Returns the default input config
    /// The sample format for JACK audio ports is always "32-bit float mono audio" unless using a custom type.
    /// The sample rate is set by the JACK server.
    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config()
    }

    /// Returns the default output config
    /// The sample format for JACK audio ports is always "32-bit float mono audio" unless using a custom type.
    /// The sample rate is set by the JACK server.
    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config()
    }

    fn build_input_stream_raw<D, E>(
        &self,
        conf: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        if !self.supports_input() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "Device does not support input",
            ));
        }
        crate::validate_stream_config(&conf)?;
        validate_sample_format(sample_format)?;

        // Keep `capture` monotonic: re-patching cpal's ports to a different hardware port
        // can raise the capture port's latency, pulling `capture` backward.
        let data_callback = crate::host::monotonic_input_callback(data_callback);
        let name = self.name.clone();
        let start_server_automatically = self.start_server_automatically;
        let connect_ports_automatically = self.connect_ports_automatically;

        let build = move || -> Result<Stream, Error> {
            let client = open_validated_client(
                &name,
                start_server_automatically,
                conf.sample_rate,
                conf.buffer_size,
            )?;
            let mut stream =
                Stream::new_input(client, conf.channels, data_callback, error_callback)?;
            if connect_ports_automatically {
                stream.connect_to_system_inputs()?;
            }
            Ok(stream)
        };

        build_with_timeout(build, timeout)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        conf: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        if !self.supports_output() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "Device does not support output",
            ));
        }
        crate::validate_stream_config(&conf)?;
        validate_sample_format(sample_format)?;

        // Keep `playback` monotonic: re-patching cpal's ports to a different hardware port
        // can lower the playback port's latency, pulling `playback` backward.
        let data_callback = crate::host::monotonic_output_callback(data_callback);
        let name = self.name.clone();
        let start_server_automatically = self.start_server_automatically;
        let connect_ports_automatically = self.connect_ports_automatically;

        let build = move || -> Result<Stream, Error> {
            let client = open_validated_client(
                &name,
                start_server_automatically,
                conf.sample_rate,
                conf.buffer_size,
            )?;
            let mut stream =
                Stream::new_output(client, conf.channels, data_callback, error_callback)?;
            if connect_ports_automatically {
                stream.connect_to_system_outputs()?;
            }
            Ok(stream)
        };

        build_with_timeout(build, timeout)
    }

    fn build_duplex_stream_raw<D, E>(
        &self,
        config: DuplexStreamConfig,
        input_sample_format: SampleFormat,
        output_sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        if !self.supports_duplex() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "Device does not support duplex streams",
            ));
        }
        validate_duplex_stream_config(&config, input_sample_format, output_sample_format)?;

        // Keep both `capture` and `playback` monotonic: re-patching cpal's ports to a
        // different hardware port can move either direction's latency, pulling `device`
        // backward.
        let data_callback = crate::host::monotonic_duplex_callback(data_callback);
        let name = self.name.clone();
        let start_server_automatically = self.start_server_automatically;
        let connect_ports_automatically = self.connect_ports_automatically;

        let build = move || -> Result<Stream, Error> {
            let client = open_validated_client(
                &name,
                start_server_automatically,
                config.sample_rate,
                config.buffer_size,
            )?;
            let mut stream = Stream::new_duplex(
                client,
                config.input_channels,
                config.output_channels,
                data_callback,
                error_callback,
            )?;
            if connect_ports_automatically {
                stream.connect_to_system_inputs()?;
                stream.connect_to_system_outputs()?;
            }
            Ok(stream)
        };

        build_with_timeout(build, timeout)
    }
}

/// Opens a fresh client under `name` and checks it against the live server's sample rate and
/// (if fixed) buffer size, since either may have drifted since the `Device` was constructed.
fn open_validated_client(
    name: &str,
    start_server_automatically: bool,
    sample_rate: SampleRate,
    buffer_size: BufferSize,
) -> Result<jack::Client, Error> {
    let client_options = super::get_client_options(start_server_automatically);
    let client = super::get_client(name, client_options)?;
    if sample_rate != client.sample_rate() {
        return Err(Error::with_message(
            ErrorKind::UnsupportedConfig,
            format!(
                "Sample rate {sample_rate} Hz does not match the server rate {} Hz",
                client.sample_rate()
            ),
        ));
    }
    if let BufferSize::Fixed(size) = buffer_size {
        if size != client.buffer_size() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "Buffer size {size} does not match the server buffer size {}",
                    client.buffer_size()
                ),
            ));
        }
    }
    Ok(client)
}

/// Runs `build` directly, or on a background thread bounded by `timeout` so a hung JACK server
/// can't block the caller indefinitely.
fn build_with_timeout(
    build: impl FnOnce() -> Result<Stream, Error> + Send + 'static,
    timeout: Option<Duration>,
) -> Result<Stream, Error> {
    let Some(dur) = timeout else {
        return build();
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tx.send(build()).ok();
    });
    rx.recv_timeout(dur).unwrap_or_else(|_| {
        Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "timed out waiting for JACK server",
        ))
    })
}

/// Applies the sample rate/buffer size/channel checks from [`crate::validate_stream_config`] to
/// each direction of a duplex configuration, and checks each direction's sample format.
fn validate_duplex_stream_config(
    config: &DuplexStreamConfig,
    input_sample_format: SampleFormat,
    output_sample_format: SampleFormat,
) -> Result<(), Error> {
    let per_direction = |channels| StreamConfig {
        channels,
        sample_rate: config.sample_rate,
        buffer_size: config.buffer_size,
    };
    crate::validate_stream_config(&per_direction(config.input_channels))?;
    validate_sample_format(input_sample_format)?;
    crate::validate_stream_config(&per_direction(config.output_channels))?;
    validate_sample_format(output_sample_format)?;
    Ok(())
}

fn validate_sample_format(sample_format: SampleFormat) -> Result<(), Error> {
    if sample_format != JACK_SAMPLE_FORMAT {
        return Err(Error::with_message(
            ErrorKind::UnsupportedConfig,
            format!(
                "Sample format {sample_format} is not supported; required format is {JACK_SAMPLE_FORMAT}"
            ),
        ));
    }
    Ok(())
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        // Device::id() can never fail in this implementation
        self.id().unwrap() == other.id().unwrap()
    }
}

impl Eq for Device {}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let desc = self.description().map_err(|_| fmt::Error)?;
        f.write_str(desc.name())
    }
}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Device::id() can never fail in this implementation
        self.id().unwrap().hash(state);
    }
}

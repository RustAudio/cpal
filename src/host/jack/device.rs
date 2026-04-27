use std::{
    hash::{Hash, Hasher},
    time::Duration,
};

use super::{stream::Stream, JACK_SAMPLE_FORMAT};
pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};
use crate::{
    traits::DeviceTrait, BufferSize, Data, DeviceDescription, DeviceDescriptionBuilder,
    DeviceDirection, DeviceId, Error, ErrorKind, InputCallbackInfo, OutputCallbackInfo,
    SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange,
};

const DEFAULT_NUM_CHANNELS: u16 = 2;
const DEFAULT_SUPPORTED_CHANNELS: [u16; 10] = [1, 2, 4, 6, 8, 16, 24, 32, 48, 64];

#[derive(Clone, Debug)]
pub struct Device {
    name: String,
    sample_rate: SampleRate,
    buffer_size: SupportedBufferSize,
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
        Ok(Self {
            // The name given to the client by JACK, could potentially be different from the name
            // supplied e.g. if there is a name collision
            name: client.name().to_string(),
            sample_rate: client.sample_rate(),
            buffer_size: SupportedBufferSize::Range {
                min: client.buffer_size(),
                max: client.buffer_size(),
            },
            direction,
            start_server_automatically,
            connect_ports_automatically,
        })
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId(crate::platform::HostId::Jack, self.name.clone()))
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

    pub fn supported_configs(&self) -> Vec<SupportedStreamConfigRange> {
        let f = match self.default_config() {
            Err(_) => return vec![],
            Ok(f) => f,
        };

        let mut supported_configs = vec![];

        for &channels in DEFAULT_SUPPORTED_CHANNELS.iter() {
            supported_configs.push(SupportedStreamConfigRange {
                channels,
                min_sample_rate: f.sample_rate,
                max_sample_rate: f.sample_rate,
                buffer_size: f.buffer_size,
                sample_format: f.sample_format,
            });
        }
        supported_configs
    }

    pub fn is_input(&self) -> bool {
        matches!(self.direction, DeviceDirection::Input)
    }

    pub fn is_output(&self) -> bool {
        matches!(self.direction, DeviceDirection::Output)
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Ok(DeviceDescriptionBuilder::new(self.name.clone())
            .direction(self.direction)
            .build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Device::id(self)
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Ok(self.supported_configs().into_iter())
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        Ok(self.supported_configs().into_iter())
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
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        if self.is_output() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "device does not support input",
            ));
        }
        if sample_format != JACK_SAMPLE_FORMAT {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!("sample format {sample_format} is not supported; JACK requires {JACK_SAMPLE_FORMAT}"),
            ));
        }

        let name = self.name.clone();
        let start_server_automatically = self.start_server_automatically;
        let connect_ports_automatically = self.connect_ports_automatically;

        let build = move || -> Result<Stream, Error> {
            let client_options = super::get_client_options(start_server_automatically);
            let client = super::get_client(&name, client_options)?;
            if conf.sample_rate != client.sample_rate() {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!(
                        "sample rate {} Hz does not match JACK server rate {} Hz",
                        conf.sample_rate,
                        client.sample_rate()
                    ),
                ));
            }
            if let BufferSize::Fixed(size) = conf.buffer_size {
                if size != client.buffer_size() {
                    return Err(Error::with_message(
                        ErrorKind::UnsupportedConfig,
                        format!(
                            "buffer size {size} does not match JACK server buffer size {}",
                            client.buffer_size()
                        ),
                    ));
                }
            }
            let mut stream =
                Stream::new_input(client, conf.channels, data_callback, error_callback)?;
            if connect_ports_automatically {
                stream.connect_to_system_inputs();
            }
            Ok(stream)
        };

        if let Some(dur) = timeout {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                tx.send(build()).ok();
            });
            match rx.recv_timeout(dur) {
                Ok(result) => result,
                Err(_) => Err(Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    "timed out waiting for JACK server",
                )),
            }
        } else {
            build()
        }
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
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        if self.is_input() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "device does not support output",
            ));
        }
        if sample_format != JACK_SAMPLE_FORMAT {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!("sample format {sample_format} is not supported; JACK requires {JACK_SAMPLE_FORMAT}"),
            ));
        }

        let name = self.name.clone();
        let start_server_automatically = self.start_server_automatically;
        let connect_ports_automatically = self.connect_ports_automatically;

        let build = move || -> Result<Stream, Error> {
            // Create a fresh client to validate against live server state.
            let client_options = super::get_client_options(start_server_automatically);
            let client = super::get_client(&name, client_options)?;
            if conf.sample_rate != client.sample_rate() {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!(
                        "sample rate {} Hz does not match JACK server rate {} Hz",
                        conf.sample_rate,
                        client.sample_rate()
                    ),
                ));
            }
            if let BufferSize::Fixed(size) = conf.buffer_size {
                if size != client.buffer_size() {
                    return Err(Error::with_message(
                        ErrorKind::UnsupportedConfig,
                        format!(
                            "buffer size {size} does not match JACK server buffer size {}",
                            client.buffer_size()
                        ),
                    ));
                }
            }
            let mut stream =
                Stream::new_output(client, conf.channels, data_callback, error_callback)?;
            if connect_ports_automatically {
                stream.connect_to_system_outputs();
            }
            Ok(stream)
        };

        if let Some(dur) = timeout {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                tx.send(build()).ok();
            });
            match rx.recv_timeout(dur) {
                Ok(result) => result,
                Err(_) => Err(Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    "timed out waiting for JACK server",
                )),
            }
        } else {
            build()
        }
    }

    fn get_channel_name(&self, channel_index: u16, input: bool) -> Result<String, Error> {
        Err(Error::UnsupportedOperation)
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        // Device::id() can never fail in this implementation
        self.id().unwrap() == other.id().unwrap()
    }
}

impl Eq for Device {}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Device::id() can never fail in this implementation
        self.id().unwrap().hash(state);
    }
}

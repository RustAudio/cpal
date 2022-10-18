use crate::traits::DeviceTrait;
use crate::{
    BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError,
    InputCallbackInfo, OutputCallbackInfo, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};
use std::hash::{Hash, Hasher};
use std::time::Duration;

use super::stream::Stream;
use super::JACK_SAMPLE_FORMAT;

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;

const DEFAULT_NUM_CHANNELS: u16 = 2;
const DEFAULT_SUPPORTED_CHANNELS: [u16; 10] = [1, 2, 4, 6, 8, 16, 24, 32, 48, 64];

/// If a device is for input or output.
/// Until we have duplex stream support JACK clients and CPAL devices for JACK will be either input or output.
#[derive(Clone, Debug)]
pub enum DeviceType {
    InputDevice,
    OutputDevice,
}
#[derive(Clone, Debug)]
pub struct Device {
    name: String,
    sample_rate: SampleRate,
    buffer_size: SupportedBufferSize,
    device_type: DeviceType,
    start_server_automatically: bool,
    connect_ports_automatically: bool,
}

impl Device {
    fn new_device(
        name: String,
        connect_ports_automatically: bool,
        start_server_automatically: bool,
        device_type: DeviceType,
    ) -> Result<Self, String> {
        // ClientOptions are bit flags that you can set with the constants provided
        let client_options = super::get_client_options(start_server_automatically);

        // Create a dummy client to find out the sample rate of the server to be able to provide it as a possible config.
        // This client will be dropped, and a new one will be created when making the stream.
        // This is a hack due to the fact that the Client must be moved to create the AsyncClient.
        match super::get_client(&name, client_options) {
            Ok(client) => Ok(Device {
                // The name given to the client by JACK, could potentially be different from the name supplied e.g.if there is a name collision
                name: client.name().to_string(),
                sample_rate: SampleRate(client.sample_rate() as u32),
                buffer_size: SupportedBufferSize::Range {
                    min: client.buffer_size(),
                    max: client.buffer_size(),
                },
                device_type,
                start_server_automatically,
                connect_ports_automatically,
            }),
            Err(e) => Err(e),
        }
    }

    pub fn default_output_device(
        name: &str,
        connect_ports_automatically: bool,
        start_server_automatically: bool,
    ) -> Result<Self, String> {
        let output_client_name = format!("{}_out", name);
        Device::new_device(
            output_client_name,
            connect_ports_automatically,
            start_server_automatically,
            DeviceType::OutputDevice,
        )
    }

    pub fn default_input_device(
        name: &str,
        connect_ports_automatically: bool,
        start_server_automatically: bool,
    ) -> Result<Self, String> {
        let input_client_name = format!("{}_in", name);
        Device::new_device(
            input_client_name,
            connect_ports_automatically,
            start_server_automatically,
            DeviceType::InputDevice,
        )
    }

    pub fn default_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let channels = DEFAULT_NUM_CHANNELS;
        let sample_rate = self.sample_rate;
        let buffer_size = self.buffer_size.clone();
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
                buffer_size: f.buffer_size.clone(),
                sample_format: f.sample_format,
            });
        }
        supported_configs
    }

    pub fn is_input(&self) -> bool {
        matches!(self.device_type, DeviceType::InputDevice)
    }

    pub fn is_output(&self) -> bool {
        matches!(self.device_type, DeviceType::OutputDevice)
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.name.clone())
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Ok(self.supported_configs().into_iter())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(self.supported_configs().into_iter())
    }

    /// Returns the default input config
    /// The sample format for JACK audio ports is always "32-bit float mono audio" unless using a custom type.
    /// The sample rate is set by the JACK server.
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config()
    }

    /// Returns the default output config
    /// The sample format for JACK audio ports is always "32-bit float mono audio" unless using a custom type.
    /// The sample rate is set by the JACK server.
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config()
    }

    fn build_input_stream_raw<D, E>(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        if let DeviceType::OutputDevice = &self.device_type {
            // Trying to create an input stream from an output device
            return Err(BuildStreamError::StreamConfigNotSupported);
        }
        if conf.sample_rate != self.sample_rate || sample_format != JACK_SAMPLE_FORMAT {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }
        // The settings should be fine, create a Client
        let client_options = super::get_client_options(self.start_server_automatically);
        let client;
        match super::get_client(&self.name, client_options) {
            Ok(c) => client = c,
            Err(e) => {
                return Err(BuildStreamError::BackendSpecific {
                    err: BackendSpecificError { description: e },
                })
            }
        };
        let mut stream = Stream::new_input(client, conf.channels, data_callback, error_callback);

        if self.connect_ports_automatically {
            stream.connect_to_system_inputs();
        }

        Ok(stream)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        if let DeviceType::InputDevice = &self.device_type {
            // Trying to create an output stream from an input device
            return Err(BuildStreamError::StreamConfigNotSupported);
        }
        if conf.sample_rate != self.sample_rate || sample_format != JACK_SAMPLE_FORMAT {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        // The settings should be fine, create a Client
        let client_options = super::get_client_options(self.start_server_automatically);
        let client;
        match super::get_client(&self.name, client_options) {
            Ok(c) => client = c,
            Err(e) => {
                return Err(BuildStreamError::BackendSpecific {
                    err: BackendSpecificError { description: e },
                })
            }
        };
        let mut stream = Stream::new_output(client, conf.channels, data_callback, error_callback);

        if self.connect_ports_automatically {
            stream.connect_to_system_outputs();
        }

        Ok(stream)
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        // Device::name() can never fail in this implementation
        self.name().unwrap() == other.name().unwrap()
    }
}

impl Eq for Device {}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name().unwrap().hash(state);
    }
}

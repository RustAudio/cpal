use crate::traits::DeviceTrait;
use crate::{
    BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError,
    InputCallbackInfo, OutputCallbackInfo, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::stream::Stream;
use super::PIPEWIRE_SAMPLE_FORMAT;

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;

const DEFAULT_NUM_CHANNELS: u16 = 2;
const DEFAULT_SUPPORTED_CHANNELS: [u16; 10] = [1, 2, 4, 6, 8, 16, 24, 32, 48, 64];

/// If a device is for input or output.
/// Until we have duplex stream support PipeWire nodes and CPAL devices for PipeWire will be either input or output.
#[derive(Clone, Debug)]
pub enum DeviceType {
    InputDevice,
    OutputDevice,
}
#[derive(Clone)]
pub struct Device {
    pub(crate) name: String,
    pub(crate) device_type: DeviceType,
    pub(crate) connect_ports_automatically: bool,
    pub(crate) client: Rc<super::conn::PWClient>,
}

impl Device {
    fn new_device(
        name: String,
        connect_ports_automatically: bool,
        device_type: DeviceType,
        client: Rc<super::conn::PWClient>,
    ) -> Result<Self, String> {
        while client
            .get_settings()
            .and_then(|s| {
                if s.allowed_sample_rates.is_empty() {
                    Err(String::new())
                } else {
                    Ok(true)
                }
            })
            .is_err()
        {}

        let settings = client.get_settings().unwrap();

        let info = client
            .create_device_node(name, device_type.clone(), connect_ports_automatically)
            .expect("Error creating device");

        Ok(Device {
            name: info.name,
            device_type,
            connect_ports_automatically,
            client,
        })
    }

    pub fn default_output_device(
        name: &str,
        connect_ports_automatically: bool,
        client: Rc<super::conn::PWClient>,
    ) -> Result<Self, String> {
        let output_client_name = format!("{}_out", name);
        Device::new_device(
            output_client_name,
            connect_ports_automatically,
            DeviceType::OutputDevice,
            client,
        )
    }

    pub fn default_input_device(
        name: &str,
        connect_ports_automatically: bool,
        client: Rc<super::conn::PWClient>,
    ) -> Result<Self, String> {
        let input_client_name = format!("{}_in", name);
        Device::new_device(
            input_client_name,
            connect_ports_automatically,
            DeviceType::InputDevice,
            client,
        )
    }

    pub fn default_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let settings = self.client.get_settings().unwrap();
        let channels = DEFAULT_NUM_CHANNELS;
        // Default is highest sample rate possible
        let sample_rate = SampleRate(*settings.allowed_sample_rates.last().unwrap());
        let buffer_size = SupportedBufferSize::Range {
            min: settings.min_buffer_size,
            max: settings.max_buffer_size,
        };
        // The sample format for JACK audio ports is always "32-bit float mono audio" in the current implementation.
        // Custom formats are allowed within JACK, but this is of niche interest.
        // The format can be found programmatically by calling jack::PortSpec::port_type() on a created port.
        let sample_format = PIPEWIRE_SAMPLE_FORMAT;
        Ok(SupportedStreamConfig {
            channels,
            sample_rate,
            buffer_size,
            sample_format,
        })
    }

    pub fn supported_configs(&self) -> Vec<SupportedStreamConfigRange> {
        let settings = self.client.get_settings().unwrap();
        let f = match self.default_config() {
            Err(_) => return vec![],
            Ok(f) => f,
        };

        let mut supported_configs = vec![];

        for &channels in DEFAULT_SUPPORTED_CHANNELS.iter() {
            supported_configs.push(SupportedStreamConfigRange {
                channels,
                min_sample_rate: SampleRate(*settings.allowed_sample_rates.first().unwrap()),
                // Default is maximum possible, so just use that
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
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let settings = self.client.get_settings().unwrap();
        if let DeviceType::OutputDevice = &self.device_type {
            // Trying to create an input stream from an output device
            return Err(BuildStreamError::StreamConfigNotSupported);
        }
        // FIXME: Not sure if we should go to the nearest neighbour sample rate
        // This issue also happens on build_output_stream_raw()
        if settings.allowed_sample_rates.contains(&conf.sample_rate.0)
            || sample_format != PIPEWIRE_SAMPLE_FORMAT
        {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        let mut stream = Stream::new_input(
            self.client.clone(),
            conf.channels,
            data_callback,
            error_callback,
        );

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
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let settings = self.client.get_settings().unwrap();
        if let DeviceType::InputDevice = &self.device_type {
            // Trying to create an output stream from an input device
            return Err(BuildStreamError::StreamConfigNotSupported);
        }
        if settings.allowed_sample_rates.contains(&conf.sample_rate.0)
            || sample_format != PIPEWIRE_SAMPLE_FORMAT
        {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        let mut stream = Stream::new_output(
            self.client.clone(),
            conf.channels,
            data_callback,
            error_callback,
        );

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

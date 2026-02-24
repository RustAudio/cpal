pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};

use super::sys;
use crate::ChannelCount;
use crate::DefaultStreamConfigError;
use crate::DeviceDescription;
use crate::DeviceDescriptionBuilder;
use crate::DeviceId;
use crate::DeviceIdError;
use crate::DeviceNameError;
use crate::DevicesError;
use crate::FrameCount;
use crate::SampleFormat;
use crate::SampleRate;
use crate::SupportedBufferSize;
use crate::SupportedStreamConfig;
use crate::SupportedStreamConfigRange;
use crate::SupportedStreamConfigsError;

use std::hash::{Hash, Hasher};
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

/// A ASIO Device
#[derive(Clone)]
pub struct Device {
    name: String,

    // Metadata cached during enumeration
    channels_in: ChannelCount,
    channels_out: ChannelCount,
    sample_rate: SampleRate,
    buffer_size_min: FrameCount,
    buffer_size_max: FrameCount,
    input_sample_format: Option<SampleFormat>,
    output_sample_format: Option<SampleFormat>,
    supported_sample_rates: Vec<SampleRate>,

    // Input and/or Output stream.
    // A driver can only have one of each.
    // They need to be created at the same time.
    pub(super) asio_streams: Arc<Mutex<sys::AsioStreams>>,
    pub(super) current_callback_flag: Arc<AtomicU32>,
}

/// All available devices.
pub struct Devices {
    asio: Arc<sys::Asio>,
    drivers: std::vec::IntoIter<String>,
    current_driver: Option<sys::Driver>,
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Device {}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl Device {
    pub fn description(&self) -> Result<DeviceDescription, DeviceNameError> {
        let direction = crate::device_description::direction_from_counts(
            Some(self.channels_in),
            Some(self.channels_out),
        );

        Ok(DeviceDescriptionBuilder::new(self.name.clone())
            .driver(self.name.clone())
            .direction(direction)
            .build())
    }

    pub fn id(&self) -> Result<DeviceId, DeviceIdError> {
        Ok(DeviceId(crate::platform::HostId::Asio, self.name.clone()))
    }

    /// Gets the supported input configs.
    /// TODO currently only supports the default.
    /// Need to find all possible configs.
    pub fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        let default = self
            .default_input_config()
            .map_err(|_| SupportedStreamConfigsError::DeviceNotAvailable)?;
        Ok(self.configs_for(default).into_iter())
    }

    /// Gets the supported output configs.
    /// TODO currently only supports the default.
    /// Need to find all possible configs.
    pub fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        let default = self
            .default_output_config()
            .map_err(|_| SupportedStreamConfigsError::DeviceNotAvailable)?;
        Ok(self.configs_for(default).into_iter())
    }

    /// Returns the default input config
    pub fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(self.channels_in, self.input_sample_format)
    }

    /// Returns the default output config
    pub fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(self.channels_out, self.output_sample_format)
    }

    fn default_config(
        &self,
        channels: ChannelCount,
        sample_format: Option<SampleFormat>,
    ) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        if channels == 0 {
            return Err(DefaultStreamConfigError::StreamTypeNotSupported);
        }
        let sample_format =
            sample_format.ok_or(DefaultStreamConfigError::StreamTypeNotSupported)?;
        Ok(SupportedStreamConfig {
            channels,
            sample_rate: self.sample_rate,
            buffer_size: SupportedBufferSize::Range {
                min: self.buffer_size_min,
                max: self.buffer_size_max,
            },
            sample_format,
        })
    }

    fn configs_for(&self, default: SupportedStreamConfig) -> Vec<SupportedStreamConfigRange> {
        let mut configs = Vec::with_capacity(default.channels as usize);
        for &rate in &self.supported_sample_rates {
            for channels in 1..=default.channels {
                configs.push(SupportedStreamConfigRange {
                    channels,
                    min_sample_rate: rate,
                    max_sample_rate: rate,
                    buffer_size: default.buffer_size,
                    sample_format: default.sample_format,
                });
            }
        }
        configs
    }
}

impl Devices {
    pub fn new(asio: Arc<sys::Asio>) -> Result<Self, DevicesError> {
        let drivers = asio.driver_names().into_iter();
        Ok(Self {
            asio,
            drivers,
            current_driver: None,
        })
    }
}

impl Iterator for Devices {
    type Item = Device;

    /// Enumerate devices by briefly loading each driver to capture its metadata.
    fn next(&mut self) -> Option<Device> {
        // Drop the previously loaded driver before attempting to load the next one.
        self.current_driver = None;

        loop {
            match self.drivers.next() {
                Some(name) => match self.asio.load_driver(&name) {
                    Ok(driver) => {
                        let Ok(channels) = driver.channels() else {
                            continue;
                        };
                        if channels.ins == 0 && channels.outs == 0 {
                            continue;
                        }

                        let Ok(sample_rate) = driver.sample_rate() else {
                            continue;
                        };
                        if sample_rate == 0.0 {
                            continue;
                        }

                        let Ok((buffer_size_min, buffer_size_max)) = driver.buffersize_range()
                        else {
                            continue;
                        };

                        let input_sample_format = driver
                            .input_data_type()
                            .ok()
                            .and_then(|t| convert_data_type(&t));
                        let output_sample_format = driver
                            .output_data_type()
                            .ok()
                            .and_then(|t| convert_data_type(&t));
                        if input_sample_format.is_none() && output_sample_format.is_none() {
                            continue;
                        }

                        let supported_sample_rates: Vec<SampleRate> = crate::COMMON_SAMPLE_RATES
                            .iter()
                            .copied()
                            .filter(|&r| driver.can_sample_rate(r.into()).unwrap_or(false))
                            .collect();
                        if supported_sample_rates.is_empty() {
                            continue;
                        }

                        self.current_driver = Some(driver);

                        let asio_streams = Arc::new(Mutex::new(sys::AsioStreams {
                            input: None,
                            output: None,
                        }));

                        return Some(Device {
                            name,
                            channels_in: channels.ins as ChannelCount,
                            channels_out: channels.outs as ChannelCount,
                            sample_rate: sample_rate as SampleRate,
                            buffer_size_min: buffer_size_min as FrameCount,
                            buffer_size_max: buffer_size_max as FrameCount,
                            input_sample_format,
                            output_sample_format,
                            supported_sample_rates,
                            asio_streams,
                            // Initialize with sentinel value so it never matches global flag state (0 or 1).
                            current_callback_flag: Arc::new(AtomicU32::new(u32::MAX)),
                        });
                    }
                    // A different driver is already loaded (e.g. an active Stream holds it). Stop
                    // cleanly rather than spinning through the rest of the list.
                    Err(sys::LoadDriverError::DriverAlreadyExists) => return None,
                    // Driver failed to load for its own reasons; skip and try the next.
                    Err(_) => continue,
                },
                None => return None,
            }
        }
    }
}

pub(crate) fn convert_data_type(ty: &sys::AsioSampleType) -> Option<SampleFormat> {
    let fmt = match *ty {
        sys::AsioSampleType::ASIOSTInt16MSB => SampleFormat::I16,
        sys::AsioSampleType::ASIOSTInt16LSB => SampleFormat::I16,
        sys::AsioSampleType::ASIOSTInt24MSB => SampleFormat::I24,
        sys::AsioSampleType::ASIOSTInt24LSB => SampleFormat::I24,
        sys::AsioSampleType::ASIOSTInt32MSB => SampleFormat::I32,
        sys::AsioSampleType::ASIOSTInt32LSB => SampleFormat::I32,
        sys::AsioSampleType::ASIOSTFloat32MSB => SampleFormat::F32,
        sys::AsioSampleType::ASIOSTFloat32LSB => SampleFormat::F32,
        sys::AsioSampleType::ASIOSTFloat64MSB => SampleFormat::F64,
        sys::AsioSampleType::ASIOSTFloat64LSB => SampleFormat::F64,
        _ => return None,
    };
    Some(fmt)
}

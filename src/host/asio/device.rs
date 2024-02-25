pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;

use super::sys;
use crate::BackendSpecificError;
use crate::DefaultStreamConfigError;
use crate::DeviceNameError;
use crate::DevicesError;
use crate::SampleFormat;
use crate::SampleRate;
use crate::SupportedBufferSize;
use crate::SupportedStreamConfig;
use crate::SupportedStreamConfigRange;
use crate::SupportedStreamConfigsError;
use std::hash::{Hash, Hasher};
use std::sync::atomic::AtomicI32;
use std::sync::{Arc, Mutex};

/// A ASIO Device
#[derive(Clone)]
pub struct Device {
    /// The driver represented by this device.
    pub driver: Arc<sys::Driver>,

    // Input and/or Output stream.
    // A driver can only have one of each.
    // They need to be created at the same time.
    pub asio_streams: Arc<Mutex<sys::AsioStreams>>,
    pub current_buffer_index: Arc<AtomicI32>,
}

/// All available devices.
pub struct Devices {
    asio: Arc<sys::Asio>,
    drivers: std::vec::IntoIter<String>,
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.driver.name() == other.driver.name()
    }
}

impl Eq for Device {}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.driver.name().hash(state);
    }
}

impl Device {
    pub fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.driver.name().to_string())
    }

    /// Gets the supported input configs.
    /// TODO currently only supports the default.
    /// Need to find all possible configs.
    pub fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        // Retrieve the default config for the total supported channels and supported sample
        // format.
        let f = match self.default_input_config() {
            Err(_) => return Err(SupportedStreamConfigsError::DeviceNotAvailable),
            Ok(f) => f,
        };

        // Collect a config for every combination of supported sample rate and number of channels.
        let mut supported_configs = vec![];
        for &rate in crate::COMMON_SAMPLE_RATES {
            if !self
                .driver
                .can_sample_rate(rate.0.into())
                .ok()
                .unwrap_or(false)
            {
                continue;
            }
            for channels in 1..f.channels + 1 {
                supported_configs.push(SupportedStreamConfigRange {
                    channels,
                    min_sample_rate: rate,
                    max_sample_rate: rate,
                    buffer_size: f.buffer_size,
                    sample_format: f.sample_format,
                })
            }
        }
        Ok(supported_configs.into_iter())
    }

    /// Gets the supported output configs.
    /// TODO currently only supports the default.
    /// Need to find all possible configs.
    pub fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        // Retrieve the default config for the total supported channels and supported sample
        // format.
        let f = match self.default_output_config() {
            Err(_) => return Err(SupportedStreamConfigsError::DeviceNotAvailable),
            Ok(f) => f,
        };

        // Collect a config for every combination of supported sample rate and number of channels.
        let mut supported_configs = vec![];
        for &rate in crate::COMMON_SAMPLE_RATES {
            if !self
                .driver
                .can_sample_rate(rate.0.into())
                .ok()
                .unwrap_or(false)
            {
                continue;
            }
            for channels in 1..f.channels + 1 {
                supported_configs.push(SupportedStreamConfigRange {
                    channels,
                    min_sample_rate: rate,
                    max_sample_rate: rate,
                    buffer_size: f.buffer_size,
                    sample_format: f.sample_format,
                })
            }
        }
        Ok(supported_configs.into_iter())
    }

    /// Returns the default input config
    pub fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let channels = self.driver.channels().map_err(default_config_err)?.ins as u16;
        let sample_rate = SampleRate(self.driver.sample_rate().map_err(default_config_err)? as _);
        let (min, max) = self.driver.buffersize_range().map_err(default_config_err)?;
        let buffer_size = SupportedBufferSize::Range {
            min: min as u32,
            max: max as u32,
        };
        // Map th ASIO sample type to a CPAL sample type
        let data_type = self.driver.input_data_type().map_err(default_config_err)?;
        let sample_format = convert_data_type(&data_type)
            .ok_or(DefaultStreamConfigError::StreamTypeNotSupported)?;
        Ok(SupportedStreamConfig {
            channels,
            sample_rate,
            buffer_size,
            sample_format,
        })
    }

    /// Returns the default output config
    pub fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let channels = self.driver.channels().map_err(default_config_err)?.outs as u16;
        let sample_rate = SampleRate(self.driver.sample_rate().map_err(default_config_err)? as _);
        let (min, max) = self.driver.buffersize_range().map_err(default_config_err)?;
        let buffer_size = SupportedBufferSize::Range {
            min: min as u32,
            max: max as u32,
        };
        let data_type = self.driver.output_data_type().map_err(default_config_err)?;
        let sample_format = convert_data_type(&data_type)
            .ok_or(DefaultStreamConfigError::StreamTypeNotSupported)?;
        Ok(SupportedStreamConfig {
            channels,
            sample_rate,
            buffer_size,
            sample_format,
        })
    }
}

impl Devices {
    pub fn new(asio: Arc<sys::Asio>) -> Result<Self, DevicesError> {
        let drivers = asio.driver_names().into_iter();
        Ok(Devices { asio, drivers })
    }
}

impl Iterator for Devices {
    type Item = Device;

    /// Load drivers and return device
    fn next(&mut self) -> Option<Device> {
        loop {
            match self.drivers.next() {
                Some(name) => match self.asio.load_driver(&name) {
                    Ok(driver) => {
                        let driver = Arc::new(driver);
                        let asio_streams = Arc::new(Mutex::new(sys::AsioStreams {
                            input: None,
                            output: None,
                        }));
                        return Some(Device {
                            driver,
                            asio_streams,
                            current_buffer_index: Arc::new(AtomicI32::new(-1)),
                        });
                    }
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
        sys::AsioSampleType::ASIOSTFloat32MSB => SampleFormat::F32,
        sys::AsioSampleType::ASIOSTFloat32LSB => SampleFormat::F32,
        sys::AsioSampleType::ASIOSTInt32MSB => SampleFormat::I32,
        sys::AsioSampleType::ASIOSTInt32LSB => SampleFormat::I32,
        _ => return None,
    };
    Some(fmt)
}

fn default_config_err(e: sys::AsioError) -> DefaultStreamConfigError {
    match e {
        sys::AsioError::NoDrivers | sys::AsioError::HardwareMalfunction => {
            DefaultStreamConfigError::DeviceNotAvailable
        }
        sys::AsioError::NoRate => DefaultStreamConfigError::StreamTypeNotSupported,
        err => {
            let description = format!("{}", err);
            BackendSpecificError { description }.into()
        }
    }
}

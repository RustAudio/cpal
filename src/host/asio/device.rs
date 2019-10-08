use std;
pub type SupportedInputFormats = std::vec::IntoIter<SupportedFormat>;
pub type SupportedOutputFormats = std::vec::IntoIter<SupportedFormat>;

use std::hash::{Hash, Hasher};
use std::sync::{Arc};
use BackendSpecificError;
use DefaultFormatError;
use DeviceNameError;
use DevicesError;
use Format;
use SampleFormat;
use SampleRate;
use SupportedFormat;
use SupportedFormatsError;
use super::sys;
use super::parking_lot::Mutex;

/// A ASIO Device
pub struct Device {
    /// The driver represented by this device.
    pub driver: Arc<sys::Driver>,

    // Input and/or Output stream.
    // An driver can only have one of each.
    // They need to be created at the same time.
    pub asio_streams: Arc<Mutex<sys::AsioStreams>>,
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

    /// Gets the supported input formats.
    /// TODO currently only supports the default.
    /// Need to find all possible formats.
    pub fn supported_input_formats(
        &self,
    ) -> Result<SupportedInputFormats, SupportedFormatsError> {
        // Retrieve the default format for the total supported channels and supported sample
        // format.
        let mut f = match self.default_input_format() {
            Err(_) => return Err(SupportedFormatsError::DeviceNotAvailable),
            Ok(f) => f,
        };

        // Collect a format for every combination of supported sample rate and number of channels.
        let mut supported_formats = vec![];
        for &rate in ::COMMON_SAMPLE_RATES {
            if !self.driver.can_sample_rate(rate.0.into()).ok().unwrap_or(false) {
                continue;
            }
            for channels in 1..f.channels + 1 {
                f.channels = channels;
                f.sample_rate = rate;
                supported_formats.push(SupportedFormat::from(f.clone()));
            }
        }
        Ok(supported_formats.into_iter())
    }

    /// Gets the supported output formats.
    /// TODO currently only supports the default.
    /// Need to find all possible formats.
    pub fn supported_output_formats(
        &self,
    ) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        // Retrieve the default format for the total supported channels and supported sample
        // format.
        let mut f = match self.default_output_format() {
            Err(_) => return Err(SupportedFormatsError::DeviceNotAvailable),
            Ok(f) => f,
        };

        // Collect a format for every combination of supported sample rate and number of channels.
        let mut supported_formats = vec![];
        for &rate in ::COMMON_SAMPLE_RATES {
            if !self.driver.can_sample_rate(rate.0.into()).ok().unwrap_or(false) {
                continue;
            }
            for channels in 1..f.channels + 1 {
                f.channels = channels;
                f.sample_rate = rate;
                supported_formats.push(SupportedFormat::from(f.clone()));
            }
        }
        Ok(supported_formats.into_iter())
    }

    /// Returns the default input format
    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        let channels = self.driver.channels().map_err(default_format_err)?.ins as u16;
        let sample_rate = SampleRate(self.driver.sample_rate().map_err(default_format_err)? as _);
        // Map th ASIO sample type to a CPAL sample type
        let data_type = self.driver.input_data_type().map_err(default_format_err)?;
        let data_type = convert_data_type(&data_type)
            .ok_or(DefaultFormatError::StreamTypeNotSupported)?;
        Ok(Format {
            channels,
            sample_rate,
            data_type,
        })
    }

    /// Returns the default output format
    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        let channels = self.driver.channels().map_err(default_format_err)?.outs as u16;
        let sample_rate = SampleRate(self.driver.sample_rate().map_err(default_format_err)? as _);
        let data_type = self.driver.output_data_type().map_err(default_format_err)?;
        let data_type = convert_data_type(&data_type)
            .ok_or(DefaultFormatError::StreamTypeNotSupported)?;
        Ok(Format {
            channels,
            sample_rate,
            data_type,
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
                        return Some(Device { driver, asio_streams });
                    }
                    Err(_) => continue,
                }
                None => return None,
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        unimplemented!()
    }
}

pub(crate) fn convert_data_type(ty: &sys::AsioSampleType) -> Option<SampleFormat> {
    let fmt = match *ty {
        sys::AsioSampleType::ASIOSTInt16MSB => SampleFormat::I16,
        sys::AsioSampleType::ASIOSTInt16LSB => SampleFormat::I16,
        sys::AsioSampleType::ASIOSTFloat32MSB => SampleFormat::F32,
        sys::AsioSampleType::ASIOSTFloat32LSB => SampleFormat::F32,
        // NOTE: While ASIO does not support these formats directly, the stream callback created by
        // CPAL supports converting back and forth between the following. This is because many ASIO
        // drivers only support `Int32` formats, while CPAL does not support this format at all. We
        // allow for this implicit conversion temporarily until CPAL gets support for an `I32`
        // format.
        sys::AsioSampleType::ASIOSTInt32MSB => SampleFormat::I16,
        sys::AsioSampleType::ASIOSTInt32LSB => SampleFormat::I16,
        _ => return None,
    };
    Some(fmt)
}

fn default_format_err(e: sys::AsioError) -> DefaultFormatError {
    match e {
        sys::AsioError::NoDrivers |
        sys::AsioError::HardwareMalfunction => DefaultFormatError::DeviceNotAvailable,
        sys::AsioError::NoRate => DefaultFormatError::StreamTypeNotSupported,
        err => {
            let description = format!("{}", err);
            BackendSpecificError { description }.into()
        }
    }
}

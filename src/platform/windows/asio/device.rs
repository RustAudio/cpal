use std;
pub type SupportedInputFormats = std::vec::IntoIter<SupportedFormat>;
pub type SupportedOutputFormats = std::vec::IntoIter<SupportedFormat>;

use std::hash::{Hash, Hasher};
use DefaultFormatError;
use Format;
use FormatsEnumerationError;
use SampleFormat;
use SampleRate;
use SupportedFormat;
use super::sys;

/// A ASIO Device
#[derive(Debug, Clone)]
pub struct Device {
    /// The drivers for this device
    pub drivers: sys::Drivers,
    /// The name of this device
    pub name: String,
}

/// All available devices
pub struct Devices {
    drivers: std::vec::IntoIter<String>,
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
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Gets the supported input formats.
    /// TODO currently only supports the default.
    /// Need to find all possible formats.
    pub fn supported_input_formats(
        &self,
    ) -> Result<SupportedInputFormats, FormatsEnumerationError> {
        // Retrieve the default format for the total supported channels and supported sample
        // format.
        let mut f = match self.default_input_format() {
            Err(_) => return Err(FormatsEnumerationError::DeviceNotAvailable),
            Ok(f) => f,
        };

        // Collect a format for every combination of supported sample rate and number of channels.
        let mut supported_formats = vec![];
        for &rate in ::COMMON_SAMPLE_RATES {
            if !self.drivers.can_sample_rate(rate.0 as u32) {
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
    ) -> Result<SupportedOutputFormats, FormatsEnumerationError> {
        // Retrieve the default format for the total supported channels and supported sample
        // format.
        let mut f = match self.default_output_format() {
            Err(_) => return Err(FormatsEnumerationError::DeviceNotAvailable),
            Ok(f) => f,
        };

        // Collect a format for every combination of supported sample rate and number of channels.
        let mut supported_formats = vec![];
        for &rate in ::COMMON_SAMPLE_RATES {
            if !self.drivers.can_sample_rate(rate.0 as u32) {
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
        let channels = self.drivers.get_channels().ins as u16;
        let sample_rate = SampleRate(self.drivers.get_sample_rate().rate);
        // Map th ASIO sample type to a CPAL sample type
        match self.drivers.get_data_type() {
            Ok(sys::AsioSampleType::ASIOSTInt16MSB) => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTInt32MSB) => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTFloat32MSB) => Ok(SampleFormat::F32),
            Ok(sys::AsioSampleType::ASIOSTInt16LSB) => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTInt32LSB) => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTFloat32LSB) => Ok(SampleFormat::F32),
            _ => Err(DefaultFormatError::StreamTypeNotSupported),
        }.map(|dt| Format {
            channels,
            sample_rate,
            data_type: dt,
        })
    }

    /// Returns the default output format
    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        let channels = self.drivers.get_channels().outs as u16;
        let sample_rate = SampleRate(self.drivers.get_sample_rate().rate);
        match self.drivers.get_data_type() {
            // Map th ASIO sample type to a CPAL sample type
            Ok(sys::AsioSampleType::ASIOSTInt16MSB) => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTFloat32MSB) => Ok(SampleFormat::F32),
            Ok(sys::AsioSampleType::ASIOSTInt16LSB) => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTInt32LSB) => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTFloat32LSB) => Ok(SampleFormat::F32),
            _ => Err(DefaultFormatError::StreamTypeNotSupported),
        }.map(|dt| Format {
            channels,
            sample_rate,
            data_type: dt,
        })
    }
}

impl Default for Devices {
    fn default() -> Devices {
        let driver_names = online_devices();
        Devices {
            drivers: driver_names.into_iter(),
        }
    }
}

impl Iterator for Devices {
    type Item = Device;

    /// Load drivers and return device
    fn next(&mut self) -> Option<Device> {
        match self.drivers.next() {
            Some(name) => sys::Drivers::load(&name)
                .or_else(|e| {
                    eprintln!("{}", e);
                    Err(e)
                }).ok()
                .map(|drivers| Device { drivers, name }),
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        unimplemented!()
    }
}

/// Asio doesn't have a concept of default
/// so returning first in list as default
pub fn default_input_device() -> Option<Device> {
    first_device()
}

/// Asio doesn't have a concept of default
/// so returning first in list as default
pub fn default_output_device() -> Option<Device> {
    first_device()
}

fn first_device() -> Option<Device> {
    let mut driver_list = online_devices();
    match driver_list.pop() {
        Some(name) => sys::Drivers::load(&name)
            .or_else(|e| {
                eprintln!("{}", e);
                Err(e)
            }).ok()
            .map(|drivers| Device { drivers, name }),
        None => None,
    }
}

/// Remove offline drivers
fn online_devices() -> Vec<String> {
    sys::get_driver_list()
        .into_iter()
        .filter(|name| sys::Drivers::load(&name).is_ok())
        .collect()
}

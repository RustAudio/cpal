extern crate asio_sys as sys;
use std;
pub type SupportedInputFormats = std::vec::IntoIter<SupportedFormat>;
pub type SupportedOutputFormats = std::vec::IntoIter<SupportedFormat>;

use Format;
use FormatsEnumerationError;
use DefaultFormatError;
use SupportedFormat;
use SampleFormat;
use SampleRate;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct Device{
    pub drivers: sys::Drivers,
    pub name: String,
}

pub struct Devices{
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
    
    // Just supporting default for now
    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, 
    FormatsEnumerationError> {
            match self.default_input_format() {
                Ok(f) => {
                let supported_formats: Vec<SupportedFormat> = [44100, 48000]
                    .into_iter()
                    .filter(|rate| self.drivers.can_sample_rate(**rate as u32) )
                    .map(|rate| {
                        let mut format = f.clone();
                        format.sample_rate = SampleRate(*rate);
                        SupportedFormat::from(format)
                    })
                    .collect();
                //Ok(vec![SupportedFormat::from(f)].into_iter())
                Ok(supported_formats.into_iter())
                },
                Err(_) => Err(FormatsEnumerationError::DeviceNotAvailable),
            }
    }

    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, 
    FormatsEnumerationError> {
        match self.default_output_format() {
            Ok(f) => {
                let supported_formats: Vec<SupportedFormat> = [44100, 48000]
                    .into_iter()
                    .filter(|rate| self.drivers.can_sample_rate(**rate as u32) )
                    .map(|rate| {
                        let mut format = f.clone();
                        format.sample_rate = SampleRate(*rate);
                        SupportedFormat::from(format)
                    })
                    .collect();
                //Ok(vec![SupportedFormat::from(f)].into_iter())
                Ok(supported_formats.into_iter())
            },
            Err(_) => Err(FormatsEnumerationError::DeviceNotAvailable),
        }
    }

    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        let channels = self.drivers.get_channels().ins as u16;
        let sample_rate = SampleRate(self.drivers.get_sample_rate().rate);
        match self.drivers.get_data_type() {
                Ok(sys::AsioSampleType::ASIOSTInt16MSB)   => Ok(SampleFormat::I16),
                Ok(sys::AsioSampleType::ASIOSTInt32MSB)   => Ok(SampleFormat::I16),
                Ok(sys::AsioSampleType::ASIOSTFloat32MSB) => Ok(SampleFormat::F32),
                Ok(sys::AsioSampleType::ASIOSTInt16LSB)   => Ok(SampleFormat::I16),
                Ok(sys::AsioSampleType::ASIOSTInt32LSB)   => Ok(SampleFormat::I16),
                Ok(sys::AsioSampleType::ASIOSTFloat32LSB) => Ok(SampleFormat::F32),		
                _ => Err(DefaultFormatError::StreamTypeNotSupported),
        }.map(|dt| {
            Format{channels,
            sample_rate, 
            data_type: dt }
        })
    }

    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        let channels = self.drivers.get_channels().outs as u16;
        let sample_rate = SampleRate(self.drivers.get_sample_rate().rate);
        match self.drivers.get_data_type() {
            Ok(sys::AsioSampleType::ASIOSTInt16MSB) => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTFloat32MSB) => Ok(SampleFormat::F32),
            Ok(sys::AsioSampleType::ASIOSTInt16LSB) => Ok(SampleFormat::I16),
            // TODO This should not be set to 16bit but is for testing
            Ok(sys::AsioSampleType::ASIOSTInt32LSB)   => Ok(SampleFormat::I16),
            Ok(sys::AsioSampleType::ASIOSTFloat32LSB) => Ok(SampleFormat::F32),		
            _ => Err(DefaultFormatError::StreamTypeNotSupported),
        }.map(|dt|{
            Format{channels,
            sample_rate, 
            data_type: dt }
        })
    }
}

impl Default for Devices {
    fn default() -> Devices {
        // Remove offline drivers
        let driver_names: Vec<String> = sys::get_driver_list()
            .into_iter()
            .filter(|name| sys::Drivers::load(&name).is_ok())
            .collect();
        Devices{ drivers: driver_names.into_iter() }

    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        match self.drivers.next() {
            Some(name) => {
                sys::Drivers::load(&name)
                    .or_else(|e| {
                            eprintln!("{}", e);
                            Err(e)
                        })
                    .ok()
                    .map(|drivers| Device{drivers, name} )
            },
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        unimplemented!()
    }
}

// Asio doesn't have a concept of default
// so returning first in list as default
pub fn default_input_device() -> Option<Device> {
    let mut driver_list = sys::get_driver_list();
    match driver_list.pop() {
        Some(name) => {
            sys::Drivers::load(&name)
                .or_else(|e| {
                        eprintln!("{}", e);
                        Err(e)
                    })
                .ok()
                .map(|drivers| Device{drivers, name} )
        },
        None => None,
    }
}

pub fn default_output_device() -> Option<Device> {
    let mut driver_list = sys::get_driver_list();
    match driver_list.pop() {
        Some(name) => {
            sys::Drivers::load(&name)
                .or_else(|e| {
                        eprintln!("{}", e);
                        Err(e)
                    })
                .ok()
                .map(|drivers| Device{drivers, name} )
        },
        None => None,
    }
}

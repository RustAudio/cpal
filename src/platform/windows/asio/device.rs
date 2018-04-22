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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Device{
    // Calls to get a driver in memory
    // require a name so I will store the
    // name here as a handle to the driver
    pub driver_name: String,
}

pub struct Devices{
    drivers: std::vec::IntoIter<String>,
}

impl Device {
    pub fn name(&self) -> String {
        self.driver_name.clone()
    }
    
    // Just supporting default for now
    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, 
    FormatsEnumerationError> {
            match self.default_input_format() {
                Ok(f) => Ok(vec![SupportedFormat::from(f)].into_iter()),
                Err(e) => Err(FormatsEnumerationError::DeviceNotAvailable),
            }
    }

    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, 
    FormatsEnumerationError> {
        match self.default_output_format() {
            Ok(f) => Ok(vec![SupportedFormat::from(f)].into_iter()),
            Err(e) => Err(FormatsEnumerationError::DeviceNotAvailable),
        }
    }

    // TODO Pass errors along
    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        let format = Format{channels: 0, sample_rate: SampleRate(0), 
            // TODO Not sure about how to set the data type
            data_type: SampleFormat::F32};

        let format = match sys::get_channels(&self.driver_name) {
            Ok(channels) => {
                Format{channels: channels.ins as u16,
                sample_rate: format.sample_rate, 
                data_type: format.data_type}
            },
            Err(e) => {
                println!("Error retrieving channels: {}", e);
                format
            },
        };


        let format = match sys::get_sample_rate(&self.driver_name) {
            Ok(sample_rate) => {
                Format{channels: format.channels,
                sample_rate: SampleRate(sample_rate.rate), 
                data_type: format.data_type}
            },
            Err(e) => {
                println!("Error retrieving sample rate: {}", e);
                format
            },
        };

        let format = match sys::get_data_type(&self.driver_name) {
            Ok(data_type) => {
                println!("Audio Type: {:?}", data_type);
                let data_type = match data_type{
                    sys::AsioSampleType::ASIOSTInt16MSB   => SampleFormat::I16,
                    sys::AsioSampleType::ASIOSTFloat32MSB => SampleFormat::F32,
                    sys::AsioSampleType::ASIOSTInt16LSB   => SampleFormat::I16,
                    // TODO This should not be set to 16bit but is for testing
                    sys::AsioSampleType::ASIOSTInt32LSB   => SampleFormat::I16,
                    sys::AsioSampleType::ASIOSTFloat32LSB => SampleFormat::F32,		
                    _ => panic!("Unsupported Audio Type: {:?}", data_type),
                };
                Format{channels: format.channels,
                sample_rate: format.sample_rate, 
                data_type: data_type}
            },
            Err(e) => {
                println!("Error retrieving sample rate: {}", e);
                format
            },
        };

        Ok(format)

    }

    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        let format = Format{channels: 0, sample_rate: SampleRate(0), 
            // TODO Not sure about how to set the data type
            data_type: SampleFormat::F32};

        let format = match sys::get_channels(&self.driver_name) {
            Ok(channels) => {
                Format{channels: channels.outs as u16,
                sample_rate: format.sample_rate, 
                data_type: format.data_type}
            },
            Err(e) => {
                println!("Error retrieving channels: {}", e);
                format
            },
        };


        let format = match sys::get_sample_rate(&self.driver_name) {
            Ok(sample_rate) => {
                Format{channels: format.channels,
                sample_rate: SampleRate(sample_rate.rate), 
                data_type: format.data_type}
            },
            Err(e) => {
                println!("Error retrieving sample rate: {}", e);
                format
            },
        };
        
        let format = match sys::get_data_type(&self.driver_name) {
            Ok(data_type) => {
                let data_type = match data_type{
                    sys::AsioSampleType::ASIOSTInt16MSB   => SampleFormat::I16,
                    sys::AsioSampleType::ASIOSTFloat32MSB => SampleFormat::F32,
                    sys::AsioSampleType::ASIOSTInt16LSB   => SampleFormat::I16,
                    // TODO This should not be set to 16bit but is for testing
                    sys::AsioSampleType::ASIOSTInt32LSB   => SampleFormat::I16,
                    sys::AsioSampleType::ASIOSTFloat32LSB => SampleFormat::F32,		
                    _ => panic!("Unsupported Audio Type: {:?}", data_type),
                };
                Format{channels: format.channels,
                sample_rate: format.sample_rate, 
                data_type: data_type}
            },
            Err(e) => {
                println!("Error retrieving sample rate: {}", e);
                format
            },
        };

        Ok(format)
    }
}

impl Default for Devices {
    fn default() -> Devices {
        Devices{ drivers: sys::get_driver_list().into_iter() }

    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        match self.drivers.next() {
            Some(dn) => Some(Device{driver_name: dn}),
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
    for dn in &driver_list{
        if dn == "ASIO4ALL v2"{
            println!("Defaulted to ASIO4ALL **remove from production**");
            return Some(Device{ driver_name: dn.clone() });
        }
    }
    match driver_list.pop() {
        Some(dn) => Some(Device{ driver_name: dn }),
        None => None,
    }
}

pub fn default_output_device() -> Option<Device> {
    let mut driver_list = sys::get_driver_list();
    // TODO For build test only,
    // remove if inproduction
    for dn in &driver_list{
        if dn == "ASIO4ALL v2"{
            println!("Defaulted to ASIO4ALL **remove from production**");
            return Some(Device{ driver_name: dn.clone() });
        }
    }
    // end remove
    match driver_list.pop() {
        Some(dn) => Some(Device{ driver_name: dn }),
        None => None,
    }
}

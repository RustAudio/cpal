/* This example aims to produce the same behaviour 
 * as the enumerate example in cpal
 * by Tom Gowan
 */

extern crate asio_sys as sys;

// This is the same data that enumerate 
// is trying to find
// Basically these are stubbed versions
//
// Format that each sample has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    // The value 0 corresponds to 0.
    I16,
    // The value 0 corresponds to 32768.
    U16,
    // The boundaries are (-1.0, 1.0).
    F32,
}
// Number of channels.
pub type ChannelCount = u16;

// The number of samples processed per second for a single channel of audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SampleRate(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub data_type: SampleFormat,
}

fn main(){
    let driver_list = sys::get_driver_list();

    let format = Format{channels: 0, sample_rate: SampleRate(0), 
        // TODO Not sure about how to set the data type
        data_type: SampleFormat::F32};
    if driver_list.len() > 0 {
        let format = match sys::get_channels(& driver_list[0]) {
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

        
        let format = match sys::get_sample_rate(& driver_list[0]) {
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

        println!("Format {:?}", format);
    }
}

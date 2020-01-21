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

fn main() {
    let asio = sys::Asio::new();
    for name in asio.driver_names() {
        println!("Driver: {:?}", name);
        let driver = asio.load_driver(&name).expect("failed to load driver");
        let channels = driver
            .channels()
            .expect("failed to retrieve channel counts");
        let sample_rate = driver
            .sample_rate()
            .expect("failed to retrieve sample rate");
        let in_fmt = Format {
            channels: channels.ins as _,
            sample_rate: SampleRate(sample_rate as _),
            data_type: SampleFormat::F32,
        };
        let out_fmt = Format {
            channels: channels.outs as _,
            sample_rate: SampleRate(sample_rate as _),
            data_type: SampleFormat::F32,
        };
        println!("  Input {:?}", in_fmt);
        println!("  Output {:?}", out_fmt);
    }
}

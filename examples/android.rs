#![allow(dead_code)]

extern crate anyhow;
extern crate cpal;

use std::iter;

use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, Transcoder, samples::{SampleBufferMut, self}, Endianness};
use cpal::{Sample, FromSample};

#[cfg_attr(target_os = "android", ndk_glue::main(backtrace = "full"))]
fn main() {
    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("failed to find output device");

    let config = device.default_output_config().unwrap();


    match config.sample_format() {
        cpal::SampleFormat::I8B1 => run::<samples::i8::B1NE>(&device, &config.into()),
        cpal::SampleFormat::I16B2(Endianness::Big) => run::<samples::i16::B2BE>(&device, &config.into()),
        cpal::SampleFormat::I16B2(Endianness::Little) => run::<samples::i16::B2LE>(&device, &config.into()),
        cpal::SampleFormat::I32B4(Endianness::Big) => run::<samples::i32::B4BE>(&device, &config.into()),
        cpal::SampleFormat::I32B4(Endianness::Little) => run::<samples::i32::B4LE>(&device, &config.into()),
        cpal::SampleFormat::I64B8(Endianness::Big) => run::<samples::i64::B8BE>(&device, &config.into()),
        cpal::SampleFormat::I64B8(Endianness::Little) => run::<samples::i64::B8LE>(&device, &config.into()),

        cpal::SampleFormat::U8B1 => run::<samples::u8::B1NE>(&device, &config.into()),
        cpal::SampleFormat::U16B2(Endianness::Big) => run::<samples::u16::B2BE>(&device, &config.into()),
        cpal::SampleFormat::U16B2(Endianness::Little) => run::<samples::u16::B2LE>(&device, &config.into()),
        cpal::SampleFormat::U32B4(Endianness::Big) => run::<samples::u32::B4BE>(&device, &config.into()),
        cpal::SampleFormat::U32B4(Endianness::Little) => run::<samples::u32::B4LE>(&device, &config.into()),
        cpal::SampleFormat::U64B8(Endianness::Big) => run::<samples::u64::B8BE>(&device, &config.into()),
        cpal::SampleFormat::U64B8(Endianness::Little) => run::<samples::u64::B8LE>(&device, &config.into()),

        cpal::SampleFormat::F32B4(Endianness::Big) => run::<samples::f32::B4BE>(&device, &config.into()),
        cpal::SampleFormat::F32B4(Endianness::Little) => run::<samples::f32::B4LE>(&device, &config.into()),
        cpal::SampleFormat::F64B8(Endianness::Big) => run::<samples::f64::B8BE>(&device, &config.into()),
        cpal::SampleFormat::F64B8(Endianness::Little) => run::<samples::f64::B8LE>(&device, &config.into()),

        sample_format => panic!("Unsupported sample format '{sample_format}'"),
    }.unwrap()

}

fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<(), anyhow::Error>
where
    T: Transcoder,
    T::Sample: FromSample<f32> ,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    // Produce a sinusoid of maximum amplitude.
    let mut sample_clock = 0f32;
    let mut next_value = move || {
        sample_clock = (sample_clock + 1.0) % sample_rate;
        (sample_clock * 440.0 * 2.0 * std::f32::consts::PI / sample_rate).sin()
    };

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

    let stream = device.build_output_stream(
        config,
        move |data: SampleBufferMut<T>, _: &cpal::OutputCallbackInfo| {
            write_data(data, channels, &mut next_value)
        },
        err_fn,
    )?;
    stream.play()?;

    std::thread::sleep(std::time::Duration::from_millis(1000));

    Ok(())
}

fn write_data<T>(output: SampleBufferMut<T>, channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: Transcoder,
    T::Sample: FromSample<f32> ,
{
    let source = iter::from_fn(|| {
        let sample = T::Sample::from_sample(next_sample());
            Some(iter::repeat(sample).take(channels))
        }).flatten();

    output.into_iter().write_iter(source);
}

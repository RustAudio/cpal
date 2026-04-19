#![allow(dead_code)]

extern crate anyhow;
extern crate cpal;

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, StreamConfig, I24,
};

#[cfg_attr(target_os = "android", ndk_glue::main(backtrace = "full"))]
fn main() {
    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("failed to find output device");

    let config = device.default_output_config().unwrap();

    match config.sample_format() {
        SampleFormat::I8 => run::<i8>(&device, config.into()).unwrap(),
        SampleFormat::I16 => run::<i16>(&device, config.into()).unwrap(),
        SampleFormat::I24 => run::<I24>(&device, config.into()).unwrap(),
        SampleFormat::I32 => run::<i32>(&device, config.into()).unwrap(),
        // SampleFormat::I48 => run::<I48>(&device, config.into()).unwrap(),
        SampleFormat::I64 => run::<i64>(&device, config.into()).unwrap(),
        SampleFormat::U8 => run::<u8>(&device, config.into()).unwrap(),
        SampleFormat::U16 => run::<u16>(&device, config.into()).unwrap(),
        // SampleFormat::U24 => run::<U24>(&device, config.into()).unwrap(),
        SampleFormat::U32 => run::<u32>(&device, config.into()).unwrap(),
        // SampleFormat::U48 => run::<U48>(&device, config.into()).unwrap(),
        SampleFormat::U64 => run::<u64>(&device, config.into()).unwrap(),
        SampleFormat::F32 => run::<f32>(&device, config.into()).unwrap(),
        SampleFormat::F64 => run::<f64>(&device, config.into()).unwrap(),
        sample_format => panic!("Unsupported sample format '{sample_format}'"),
    }
}

fn run<T>(device: &Device, config: StreamConfig) -> Result<(), anyhow::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate as f32;
    let channels = config.channels as usize;

    // Produce a sinusoid of maximum amplitude.
    let mut sample_clock = 0f32;
    let mut next_value = move || {
        sample_clock = (sample_clock + 1.0) % sample_rate;
        (sample_clock * 440.0 * 2.0 * std::f32::consts::PI / sample_rate).sin()
    };

    let err_fn = |err| eprintln!("an error occurred on stream: {err}");

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &OutputCallbackInfo| {
            write_data(data, channels, &mut next_value)
        },
        err_fn,
        None,
    )?;
    stream.play()?;

    std::thread::sleep(std::time::Duration::from_millis(1000));

    Ok(())
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let value: T = T::from_sample(next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

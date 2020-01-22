extern crate anyhow;
extern crate cpal;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn main() -> Result<(), anyhow::Error> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("failed to find a default output device");
    let format = device.default_output_format()?;
    let sample_rate = format.sample_rate.0 as f32;
    let channels = format.channels as usize;

    // Produce a sinusoid of maximum amplitude.
    let mut sample_clock = 0f32;
    let mut next_value = move || {
        sample_clock = (sample_clock + 1.0) % sample_rate;
        (sample_clock * 440.0 * 2.0 * 3.141592 / sample_rate).sin()
    };

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

    let stream = match format.data_type {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &format.shape(),
            move |data: &mut [f32]| write_data(data, channels, &mut next_value),
            err_fn,
        )?,
        cpal::SampleFormat::I16 => device.build_output_stream(
            &format.shape(),
            move |data: &mut [i16]| write_data(data, channels, &mut next_value),
            err_fn,
        )?,
        cpal::SampleFormat::U16 => device.build_output_stream(
            &format.shape(),
            move |data: &mut [u16]| write_data(data, channels, &mut next_value),
            err_fn,
        )?,
    };

    stream.play()?;

    std::thread::sleep(std::time::Duration::from_millis(1000));

    Ok(())
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: cpal::Sample,
{
    for frame in output.chunks_mut(channels) {
        let value: T = cpal::Sample::from::<f32>(&next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

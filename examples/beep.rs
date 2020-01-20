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

    let data_fn = move |data: &mut cpal::Data| match data.sample_format() {
        cpal::SampleFormat::F32 => write_data::<f32>(data, channels, &mut next_value),
        cpal::SampleFormat::I16 => write_data::<i16>(data, channels, &mut next_value),
        cpal::SampleFormat::U16 => write_data::<u16>(data, channels, &mut next_value),
    };

    let stream = device.build_output_stream(&format, data_fn, err_fn)?;

    stream.play()?;

    std::thread::sleep(std::time::Duration::from_millis(1000));

    Ok(())
}

fn write_data<T>(output: &mut cpal::Data, channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: cpal::Sample,
{
    let output = output.as_slice_mut::<T>().expect("unexpected sample type");
    for frame in output.chunks_mut(channels) {
        let value: T = cpal::Sample::from::<f32>(&next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

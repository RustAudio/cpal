//! Records a WAV file (roughly 3 seconds long) using the default input device and format.
//!
//! The input data is recorded to "$CARGO_MANIFEST_DIR/recorded.wav".

extern crate anyhow;
extern crate cpal;
extern crate hound;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn main() -> Result<(), anyhow::Error> {
    // Use the default host for working with audio devices.
    let host = cpal::default_host();

    // Setup the default input device and stream with the default input format.
    let device = host
        .default_input_device()
        .expect("Failed to get default input device");
    println!("Default input device: {}", device.name()?);
    let format = device
        .default_input_format()
        .expect("Failed to get default input format");
    println!("Default input format: {:?}", format);
    // The WAV file we're recording to.
    const PATH: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/recorded.wav");
    let spec = wav_spec_from_format(&format);
    let writer = hound::WavWriter::create(PATH, spec)?;
    let writer = std::sync::Arc::new(std::sync::Mutex::new(Some(writer)));

    // A flag to indicate that recording is in progress.
    println!("Begin recording...");

    // Run the input stream on a separate thread.
    let writer_2 = writer.clone();
    let stream = device.build_input_stream(&format, move |event| {
        let data = match event {
            Ok(data) => data,
            Err(err) => {
                eprintln!("an error occurred on stream: {}", err);
                return;
            },
        };

        // Otherwise write to the wav writer.
        match data {
            cpal::StreamData::Input {
                buffer: cpal::UnknownTypeInputBuffer::U16(buffer),
            } => {
                if let Ok(mut guard) = writer_2.try_lock() {
                    if let Some(writer) = guard.as_mut() {
                        for sample in buffer.iter() {
                            let sample = cpal::Sample::to_i16(sample);
                            writer.write_sample(sample).ok();
                        }
                    }
                }
            },
            cpal::StreamData::Input {
                buffer: cpal::UnknownTypeInputBuffer::I16(buffer),
            } => {
                if let Ok(mut guard) = writer_2.try_lock() {
                    if let Some(writer) = guard.as_mut() {
                        for &sample in buffer.iter() {
                            writer.write_sample(sample).ok();
                        }
                    }
                }
            },
            cpal::StreamData::Input {
                buffer: cpal::UnknownTypeInputBuffer::F32(buffer),
            } => {
                if let Ok(mut guard) = writer_2.try_lock() {
                    if let Some(writer) = guard.as_mut() {
                        for &sample in buffer.iter() {
                            writer.write_sample(sample).ok();
                        }
                    }
                }
            },
            _ => (),
        }
    })?;
    stream.play()?;

    // Let recording go for roughly three seconds.
    std::thread::sleep(std::time::Duration::from_secs(3));
    drop(stream);
    writer.lock().unwrap().take().unwrap().finalize()?;
    println!("Recording {} complete!", PATH);
    Ok(())
}

fn sample_format(format: cpal::SampleFormat) -> hound::SampleFormat {
    match format {
        cpal::SampleFormat::U16 => hound::SampleFormat::Int,
        cpal::SampleFormat::I16 => hound::SampleFormat::Int,
        cpal::SampleFormat::F32 => hound::SampleFormat::Float,
    }
}

fn wav_spec_from_format(format: &cpal::Format) -> hound::WavSpec {
    hound::WavSpec {
        channels: format.channels as _,
        sample_rate: format.sample_rate.0 as _,
        bits_per_sample: (format.data_type.sample_size() * 8) as _,
        sample_format: sample_format(format.data_type),
    }
}

//! Feeds back the input stream directly into the output stream.
//!
//! Assumes that the input and output devices can use the same stream format and that they support
//! the f32 sample format.
//!
//! Uses a delay of `LATENCY_MS` milliseconds in case the default input and output streams are not
//! precisely synchronised.

extern crate anyhow;
extern crate cpal;
extern crate ringbuf;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::RingBuffer;

const LATENCY_MS: f32 = 150.0;

fn main() -> Result<(), anyhow::Error> {
    let host = cpal::default_host();

    // Default devices.
    let input_device = host
        .default_input_device()
        .expect("failed to get default input device");
    let output_device = host
        .default_output_device()
        .expect("failed to get default output device");
    println!("Using default input device: \"{}\"", input_device.name()?);
    println!("Using default output device: \"{}\"", output_device.name()?);

    // We'll try and use the same format between streams to keep it simple
    let mut format = input_device.default_input_format()?;
    format.data_type = cpal::SampleFormat::F32;

    // Create a delay in case the input and output devices aren't synced.
    let latency_frames = (LATENCY_MS / 1_000.0) * format.sample_rate.0 as f32;
    let latency_samples = latency_frames as usize * format.channels as usize;

    // The buffer to share samples
    let ring = RingBuffer::new(latency_samples * 2);
    let (mut producer, mut consumer) = ring.split();

    // Fill the samples with 0.0 equal to the length of the delay.
    for _ in 0..latency_samples {
        // The ring buffer has twice as much space as necessary to add latency here,
        // so this should never fail
        producer.push(0.0).unwrap();
    }

    // Build streams.
    println!("Attempting to build both streams with `{:?}`.", format);
    let input_stream = input_device.build_input_stream(&format, move |result| {
        let data = match result {
            Ok(data) => data,
            Err(err) => {
                eprintln!("an error occurred on input stream: {}", err);
                return;
            },
        };

        match data {
            cpal::StreamData::Input {
                buffer: cpal::UnknownTypeInputBuffer::F32(buffer),
            } => {
                let mut output_fell_behind = false;
                for &sample in buffer.iter() {
                    if producer.push(sample).is_err() {
                        output_fell_behind = true;
                    }
                }
                if output_fell_behind {
                    eprintln!("output stream fell behind: try increasing latency");
                }
            },
            _ => panic!("Expected input with f32 data"),
        }
    })?;
    let output_stream = output_device.build_output_stream(&format, move |result| {
        let data = match result {
            Ok(data) => data,
            Err(err) => {
                eprintln!("an error occurred on output stream: {}", err);
                return;
            },
        };
        match data {
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::F32(mut buffer),
            } => {
                let mut input_fell_behind = None;
                for sample in buffer.iter_mut() {
                    *sample = match consumer.pop() {
                        Ok(s) => s,
                        Err(err) => {
                            input_fell_behind = Some(err);
                            0.0
                        },
                    };
                }
                if let Some(err) = input_fell_behind {
                    eprintln!("input stream fell behind: {:?}: try increasing latency", err);
                }
            },
            _ => panic!("Expected output with f32 data"),
        }
    })?;
    println!("Successfully built streams.");

    // Play the streams.
    println!(
        "Starting the input and output streams with `{}` milliseconds of latency.",
        LATENCY_MS
    );
    input_stream.play()?;
    output_stream.play()?;

    // Run for 3 seconds before closing.
    println!("Playing for 3 seconds... ");
    std::thread::sleep(std::time::Duration::from_secs(3));
    drop(input_stream);
    drop(output_stream);
    println!("Done!");
    Ok(())
}

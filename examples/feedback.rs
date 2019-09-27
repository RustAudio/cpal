//! Feeds back the input stream directly into the output stream.
//!
//! Assumes that the input and output devices can use the same stream format and that they support
//! the f32 sample format.
//!
//! Uses a delay of `LATENCY_MS` milliseconds in case the default input and output streams are not
//! precisely synchronised.

extern crate cpal;
extern crate failure;
extern crate ringbuf;

use cpal::traits::{DeviceTrait, EventLoopTrait, HostTrait};
use ringbuf::RingBuffer;

const LATENCY_MS: f32 = 150.0;

fn main() -> Result<(), failure::Error> {
    let host = cpal::default_host();
    let event_loop = host.event_loop();

    // Default devices.
    let input_device = host.default_input_device().expect("failed to get default input device");
    let output_device = host.default_output_device().expect("failed to get default output device");
    println!("Using default input device: \"{}\"", input_device.name()?);
    println!("Using default output device: \"{}\"", output_device.name()?);

    // We'll try and use the same format between streams to keep it simple
    let mut format = input_device.default_input_format()?;
    format.data_type = cpal::SampleFormat::F32;

    // Build streams.
    println!("Attempting to build both streams with `{:?}`.", format);
    let input_stream_id = event_loop.build_input_stream(&input_device, &format)?;
    let output_stream_id = event_loop.build_output_stream(&output_device, &format)?;
    println!("Successfully built streams.");

    // Create a delay in case the input and output devices aren't synced.
    let latency_frames = (LATENCY_MS / 1_000.0) * format.sample_rate.0 as f32;
    let latency_samples = latency_frames as usize * format.channels as usize;

    // The buffer to share samples
    let ring = RingBuffer::new(latency_samples * 2);
    let (mut producer, mut consumer) = ring.split();

    // Fill the samples with 0.0 equal to the length of the delay.
    for _ in 0..latency_samples {
        producer.push(0.0).unwrap();
    }

    // Play the streams.
    println!("Starting the input and output streams with `{}` milliseconds of latency.", LATENCY_MS);
    event_loop.play_stream(input_stream_id.clone())?;
    event_loop.play_stream(output_stream_id.clone())?;

    // Run the event loop on a separate thread.
    std::thread::spawn(move || {
        event_loop.run(move |id, result| {
            let data = match result {
                Ok(data) => data,
                Err(err) => {
                    eprintln!("an error occurred on stream {:?}: {}", id, err);
                    return;
                }
            };

            match data {
                cpal::StreamData::Input { buffer: cpal::UnknownTypeInputBuffer::F32(buffer) } => {
                    assert_eq!(id, input_stream_id);
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
                cpal::StreamData::Output { buffer: cpal::UnknownTypeOutputBuffer::F32(mut buffer) } => {
                    assert_eq!(id, output_stream_id);
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
                    if let Some(_) = input_fell_behind {
                        eprintln!("input stream fell behind: try increasing latency");
                    }
                },
                _ => panic!("we're expecting f32 data"),
            }
        });
    });

    // Run for 3 seconds before closing.
    println!("Playing for 3 seconds... ");
    std::thread::sleep(std::time::Duration::from_secs(3));
    println!("Done!");
    Ok(())
}

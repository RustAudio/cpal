//! Feeds back the input stream directly into the output stream.
//!
//! Assumes that the input and output devices can use the same stream format and that they support
//! the f32 sample format.
//!
//! Uses a delay of `LATENCY_MS` milliseconds in case the default input and output streams are not
//! precisely synchronised.

extern crate cpal;

const LATENCY_MS: f32 = 150.0;

fn main() {
    let event_loop = cpal::EventLoop::new();

    // Default devices.
    let input_device = cpal::default_input_device().expect("Failed to get default input device");
    let output_device = cpal::default_output_device().expect("Failed to get default output device");
    println!("Using default input device: \"{}\"", input_device.name().unwrap());
    println!("Using default output device: \"{}\"", output_device.name().unwrap());

    // We'll try and use the same format between streams to keep it simple
    let mut format = input_device.default_input_format().expect("Failed to get default format");
    format.data_type = cpal::SampleFormat::F32;

    // Build streams.
    println!("Attempting to build both streams with `{:?}`.", format);
    let input_stream_id = event_loop.build_input_stream(&input_device, &format).unwrap();
    let output_stream_id = event_loop.build_output_stream(&output_device, &format).unwrap();
    println!("Successfully built streams.");

    // Create a delay in case the input and output devices aren't synced.
    let latency_frames = (LATENCY_MS / 1_000.0) * format.sample_rate.0 as f32;
    let latency_samples = latency_frames as usize * format.channels as usize;

    // The channel to share samples.
    let (tx, rx) = std::sync::mpsc::sync_channel(latency_samples * 2);

    // Fill the samples with 0.0 equal to the length of the delay.
    for _ in 0..latency_samples {
        tx.send(0.0).unwrap();
    }

    // Play the streams.
    println!("Starting the input and output streams with `{}` milliseconds of latency.", LATENCY_MS);
    event_loop.play_stream(input_stream_id.clone());
    event_loop.play_stream(output_stream_id.clone());

    // Run the event loop on a separate thread.
    std::thread::spawn(move || {
        event_loop.run(move |id, data| {
            match data {
                cpal::StreamData::Input { buffer: cpal::UnknownTypeInputBuffer::F32(buffer) } => {
                    assert_eq!(id, input_stream_id);
                    let mut output_fell_behind = false;
                    for &sample in buffer.iter() {
                        if tx.try_send(sample).is_err() {
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
                        *sample = match rx.try_recv() {
                            Ok(s) => s,
                            Err(err) => {
                                input_fell_behind = Some(err);
                                0.0
                            },
                        };
                    }
                    if let Some(err) = input_fell_behind {
                        eprintln!("input stream fell behind: {}: try increasing latency", err);
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
}

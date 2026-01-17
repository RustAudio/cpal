//! Feeds back the input stream directly into the output stream using a duplex stream.
//!
//! Unlike the `feedback.rs` example which uses separate input/output streams with a ring buffer,
//! duplex streams provide hardware-synchronized input/output without additional buffering.
//!
//! Note: Currently only supported on macOS (CoreAudio). Windows (WASAPI) and Linux (ALSA)
//! implementations are planned.

use clap::Parser;
use cpal::duplex::DuplexStreamConfig;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::BufferSize;

#[derive(Parser, Debug)]
#[command(version, about = "CPAL duplex feedback example", long_about = None)]
struct Opt {
    /// The audio device to use (must support duplex operation)
    #[arg(short, long, value_name = "DEVICE")]
    device: Option<String>,

    /// Number of input channels
    #[arg(long, value_name = "CHANNELS", default_value_t = 2)]
    input_channels: u16,

    /// Number of output channels
    #[arg(long, value_name = "CHANNELS", default_value_t = 2)]
    output_channels: u16,

    /// Sample rate in Hz
    #[arg(short, long, value_name = "RATE", default_value_t = 48000)]
    sample_rate: u32,

    /// Buffer size in frames
    #[arg(short, long, value_name = "FRAMES", default_value_t = 512)]
    buffer_size: u32,
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();
    let host = cpal::default_host();

    // Find the device.
    let device = if let Some(device_name) = opt.device {
        let id = &device_name.parse().expect("failed to parse device id");
        host.device_by_id(id)
    } else {
        host.default_output_device()
    }
    .expect("failed to find device");

    println!("Using device: \"{}\"", device.id()?);

    // Create duplex stream configuration.
    let config = DuplexStreamConfig::new(
        opt.input_channels,
        opt.output_channels,
        opt.sample_rate,
        BufferSize::Fixed(opt.buffer_size),
    );

    println!("Building duplex stream with config: {config:?}");

    let stream = device.build_duplex_stream::<f32, _, _>(
        &config,
        move |input, output, _info| {
            output.fill(0.0);
            let copy_len = input.len().min(output.len());
            output[..copy_len].copy_from_slice(&input[..copy_len]);
        },
        |err| eprintln!("Stream error: {err}"),
        None,
    )?;

    println!("Successfully built duplex stream.");
    println!(
        "Input: {} channels, Output: {} channels, Sample rate: {} Hz, Buffer size: {} frames",
        opt.input_channels, opt.output_channels, opt.sample_rate, opt.buffer_size
    );

    println!("Starting duplex stream...");
    stream.play()?;

    println!("Playing for 10 seconds... (speak into your microphone)");
    std::thread::sleep(std::time::Duration::from_secs(10));

    drop(stream);
    println!("Done!");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Duplex streams are currently only supported on macOS.");
    eprintln!("Windows (WASAPI) and Linux (ALSA) support is planned.");
}

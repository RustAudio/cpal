//! Feeds back the input stream directly into the output stream using a duplex stream.
//!
//! Unlike the `feedback.rs` example which uses separate input/output streams with a ring buffer,
//! duplex streams provide hardware-synchronized input/output without additional buffering.
//!
//! Note: Currently only supported on macOS (CoreAudio). Windows (WASAPI) and Linux (ALSA)
//! implementations are planned.

#[cfg(target_os = "macos")]
mod imp {
    use clap::Parser;
    use cpal::duplex::DuplexStreamConfig;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{BufferSize, ChannelCount, FrameCount, Sample, SampleRate};

    #[derive(Parser, Debug)]
    #[command(version, about = "CPAL duplex feedback example", long_about = None)]
    struct Opt {
        /// The audio device to use (must support duplex operation)
        #[arg(short, long, value_name = "DEVICE")]
        device: Option<String>,

        /// Number of input channels
        #[arg(long, value_name = "CHANNELS", default_value_t = 2)]
        input_channels: ChannelCount,

        /// Number of output channels
        #[arg(long, value_name = "CHANNELS", default_value_t = 2)]
        output_channels: ChannelCount,

        /// Sample rate in Hz
        #[arg(short, long, value_name = "RATE", default_value_t = 48000)]
        sample_rate: SampleRate,

        /// Buffer size in frames (omit for device default)
        #[arg(short, long, value_name = "FRAMES")]
        buffer_size: Option<FrameCount>,
    }

    pub fn run() -> anyhow::Result<()> {
        let opt = Opt::parse();
        let host = cpal::default_host();

        // Find the device by device ID or use default
        let device = match opt.device {
            Some(device_id_str) => {
                let device_id = device_id_str.parse().expect("failed to parse device id");
                host.device_by_id(&device_id)
                    .expect(&format!("failed to find device with id: {}", device_id_str))
            }
            None => host
                .default_output_device()
                .expect("no default output device"),
        };

        println!("Using device: \"{}\"", device.description()?.name());

        // Create duplex stream configuration.
        let config = DuplexStreamConfig {
            input_channels: opt.input_channels,
            output_channels: opt.output_channels,
            sample_rate: opt.sample_rate,
            buffer_size: opt
                .buffer_size
                .map(|s| BufferSize::Fixed(s))
                .unwrap_or(BufferSize::Default),
        };

        println!("Building duplex stream with config: {config:?}");

        let stream = device.build_duplex_stream::<f32, _, _>(
            &config,
            move |input, output, _info| {
                output.fill(Sample::EQUILIBRIUM);
                let copy_len = input.len().min(output.len());
                output[..copy_len].copy_from_slice(&input[..copy_len]);
            },
            |err| eprintln!("Stream error: {err}"),
            None,
        )?;

        println!("Successfully built duplex stream.");
        println!(
            "Input: {} channels, Output: {} channels, Sample rate: {} Hz, Buffer size: {:?} frames",
            opt.input_channels, opt.output_channels, opt.sample_rate, opt.buffer_size
        );

        println!("Starting duplex stream...");
        stream.play()?;

        println!("Playing for 10 seconds... (speak into your microphone)");
        std::thread::sleep(std::time::Duration::from_secs(10));

        println!("Done!");
        Ok(())
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    imp::run().unwrap();

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("Duplex streams are not supported on this platform.");
    }
}

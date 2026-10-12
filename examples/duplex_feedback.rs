// Duplex feedback example.

#[cfg(target_os = "macos")]
mod imp {
    use clap::Parser;
    use cpal::{
        BufferSize, ChannelCount, DuplexStreamConfig, FrameCount, Sample, SampleRate,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    };

    #[derive(Parser, Debug)]
    #[command(version, about = "CPAL duplex feedback example", long_about = None)]
    struct Opt {
        /// List devices that can build duplex streams, then exit.
        #[arg(long)]
        list: bool,

        /// Device ID to use for the duplex stream. If omitted, the default output device is
        /// used.
        #[arg(short, long, value_name = "ID")]
        device: Option<String>,

        /// Number of input channels to capture.
        #[arg(long, default_value_t = 1)]
        input_channels: ChannelCount,

        /// Number of output channels to render.
        #[arg(long, default_value_t = 2)]
        output_channels: ChannelCount,

        /// Sample rate.
        #[arg(long, default_value_t = 48_000)]
        sample_rate: SampleRate,

        /// Optional fixed buffer size, in frames. Omit to use the device default.
        #[arg(long)]
        buffer_size: Option<FrameCount>,
    }

    pub fn run() -> Result<(), cpal::Error> {
        let opt = Opt::parse();
        let host = cpal::default_host();

        if opt.list {
            return list_duplex_devices(&host);
        }

        let device = match opt.device.as_deref() {
            Some(id_str) => {
                let id = id_str.parse().map_err(|e| {
                    cpal::Error::with_message(
                        cpal::ErrorKind::InvalidInput,
                        format!("failed to parse device id {id_str:?}: {e}"),
                    )
                })?;
                host.device_by_id(&id).ok_or_else(|| {
                    cpal::Error::with_message(
                        cpal::ErrorKind::DeviceNotAvailable,
                        format!("no device with id {id_str:?}"),
                    )
                })?
            }
            None => host.default_output_device().ok_or_else(|| {
                cpal::Error::with_message(
                    cpal::ErrorKind::DeviceNotAvailable,
                    "no default output device",
                )
            })?,
        };

        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());
        println!("using device: {device_name}");

        if !device.supports_duplex() {
            return Err(cpal::Error::with_message(
                cpal::ErrorKind::UnsupportedOperation,
                "this device does not support duplex streams \
                 (run with --list to see candidates)",
            ));
        }

        let config = DuplexStreamConfig {
            input_channels: opt.input_channels,
            output_channels: opt.output_channels,
            sample_rate: opt.sample_rate,
            buffer_size: opt
                .buffer_size
                .map_or(BufferSize::Default, BufferSize::Fixed),
        };

        let input_channels = opt.input_channels;
        let output_channels = opt.output_channels;

        let stream = device.build_duplex_stream::<f32, f32, _, _>(
            config,
            move |input, output, _info| {
                mix_to_output(input, output, input_channels, output_channels)
            },
            |err| eprintln!("duplex stream error: {err}"),
            None,
        )?;

        stream.start()?;

        println!("playing duplex feedback. Ctrl-C to exit.");
        std::thread::park();
        Ok(())
    }

    /// Mix interleaved input frames to mono and broadcast across all output channels,
    /// padding any trailing output samples with silence.
    fn mix_to_output(
        input: &[f32],
        output: &mut [f32],
        input_channels: ChannelCount,
        output_channels: ChannelCount,
    ) {
        let input_channels = input_channels as usize;
        let output_channels = output_channels as usize;
        let input_frames = input.len() / input_channels.max(1);
        let output_frames = output.len() / output_channels.max(1);
        let frames = input_frames.min(output_frames);

        for frame in 0..frames {
            let mut acc = f32::EQUILIBRIUM;
            for ch in 0..input_channels {
                acc += input[frame * input_channels + ch];
            }
            let mixed = acc / input_channels.max(1) as f32;
            for ch in 0..output_channels {
                output[frame * output_channels + ch] = mixed;
            }
        }

        output[frames * output_channels..].fill(f32::EQUILIBRIUM);
    }

    /// Print the devices on the active host that report `supports_duplex() == true`, with their
    /// IDs (for use with `--device`) and descriptions.
    fn list_duplex_devices(host: &cpal::Host) -> Result<(), cpal::Error> {
        let default_id = host.default_output_device().and_then(|d| d.id().ok());

        let mut found = false;
        println!("Devices supporting duplex on this host:");
        for device in host.devices()? {
            if !device.supports_duplex() {
                continue;
            }
            found = true;
            let id = device.id().ok();
            let name = device
                .description()
                .map_or_else(|_| "<unknown>".to_string(), |d| d.name().to_string());
            let default_marker = if id.is_some() && id == default_id {
                " [default]"
            } else {
                ""
            };
            let id_str = id
                .as_ref()
                .map_or_else(|| "<no id>".to_string(), ToString::to_string);
            println!("  {id_str}{default_marker}  —  {name}");
        }
        if !found {
            println!("  (none)");
        }
        Ok(())
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    if let Err(e) = imp::run() {
        eprintln!("duplex_feedback: {e}");
        std::process::exit(1);
    }

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("duplex streams are not supported on this platform");
    }
}

//! Feeds the input stream directly back into the output stream, from a single duplex callback.
//!
//! A duplex stream drives both directions from one device callback on one clock, so neither the
//! ring buffer nor the delay is needed. It requires a device that [`DeviceTrait::supports_duplex`].
//!
//! Where a platform exposes no natively duplex device, you may be able to compose one on the
//! system: using an Aggregate Device on macOS (with Audio MIDI Setup), or an `asym` PCM in
//! `~/.asoundrc` on ALSA.
//!
//! For simplicity this example requires both directions to use the same sample format, though
//! [`DeviceTrait::build_duplex_stream`] does not.

use clap::Parser;
use cpal::{
    Device, DuplexCallbackInfo, DuplexStreamConfig, Error, ErrorKind, HostId, SampleFormat,
    SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

#[derive(Parser, Debug)]
#[command(version, about = "CPAL duplex example", long_about = None)]
struct Opt {
    /// The duplex audio device to use. Defaults to the first device reporting duplex support.
    #[arg(short, long, value_name = "DEVICE")]
    device: Option<String>,

    /// Use the JACK host. Requires `--features jack`.
    #[arg(long, default_value_t = false)]
    jack: bool,

    /// Use the PipeWire host. Requires `--features pipewire`.
    #[arg(long, default_value_t = false)]
    pipewire: bool,

    /// Use the ASIO host. Requires `--features asio`.
    #[arg(long, default_value_t = false)]
    asio: bool,
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    // JACK/PipeWire/ASIO support must be enabled at compile time, and is
    // only available on some platforms.
    #[allow(unused_mut, unused_assignments)]
    let mut jack_host_id: Result<HostId, Error> = Err(ErrorKind::HostUnavailable.into());
    #[allow(unused_mut, unused_assignments)]
    let mut pipewire_host_id: Result<HostId, Error> = Err(ErrorKind::HostUnavailable.into());
    #[allow(unused_mut, unused_assignments)]
    let mut asio_host_id: Result<HostId, Error> = Err(ErrorKind::HostUnavailable.into());

    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ))]
    {
        #[cfg(feature = "jack")]
        {
            jack_host_id = Ok(HostId::Jack);
        }

        #[cfg(feature = "pipewire")]
        {
            pipewire_host_id = Ok(HostId::PipeWire);
        }
    }

    #[cfg(target_os = "windows")]
    {
        #[cfg(feature = "asio")]
        {
            asio_host_id = Ok(HostId::Asio);
        }
    }

    // Manually check for flags. Can be passed through cargo with -- e.g.
    // cargo run --release --example duplex --features jack -- --jack
    let host = if opt.jack {
        jack_host_id
            .and_then(cpal::host_from_id)
            .expect("make sure `--features jack` is specified, and the platform is supported")
    } else if opt.pipewire {
        pipewire_host_id
            .and_then(cpal::host_from_id)
            .expect("make sure `--features pipewire` is specified, and the platform is supported")
    } else if opt.asio {
        asio_host_id
            .and_then(cpal::host_from_id)
            .expect("make sure `--features asio` is specified, and the platform is supported")
    } else {
        cpal::default_host()
    };

    let device = if let Some(device) = opt.device {
        let id = &device.parse().expect("failed to parse device id");
        host.device_by_id(id).expect("failed to find device")
    } else {
        host.devices()?
            .find(|device| device.supports_duplex())
            .ok_or_else(|| anyhow::anyhow!("no device on this host reports duplex support."))?
    };

    if !device.supports_duplex() {
        anyhow::bail!(
            "device \"{}\" does not support duplex streams",
            device.id()?
        );
    }
    println!("Using duplex device: \"{}\"", device.id()?);

    let input_config = device.default_input_config()?;
    let output_config = device.default_output_config()?;
    assert_eq!(
        input_config.sample_format(),
        output_config.sample_format(),
        "both directions must share a sample format for this example"
    );

    let config = device.default_duplex_config()?;

    match input_config.sample_format() {
        SampleFormat::I8 => run::<i8>(&device, config),
        SampleFormat::I16 => run::<i16>(&device, config),
        SampleFormat::I32 => run::<i32>(&device, config),
        SampleFormat::I64 => run::<i64>(&device, config),
        SampleFormat::U8 => run::<u8>(&device, config),
        SampleFormat::U16 => run::<u16>(&device, config),
        SampleFormat::U32 => run::<u32>(&device, config),
        SampleFormat::U64 => run::<u64>(&device, config),
        SampleFormat::F32 => run::<f32>(&device, config),
        SampleFormat::F64 => run::<f64>(&device, config),
        sample_format => panic!("Unsupported sample format '{sample_format}'"),
    }
}

fn run<T>(device: &Device, config: DuplexStreamConfig) -> anyhow::Result<()>
where
    T: SizedSample + Send + 'static,
{
    let input_channels = config.input_channels as usize;
    let output_channels = config.output_channels as usize;

    println!(
        "Attempting to build a duplex stream with {} samples and `{config:?}`.",
        T::FORMAT
    );
    let shared_channels = input_channels.min(output_channels);
    let stream = device.build_duplex_stream(
        config,
        move |input: &[T], output: &mut [T], _: &DuplexCallbackInfo| {
            // Both directions arrive together, so captured audio goes straight out with nothing
            // buffered in between.
            //
            // The channel counts do not need to match. The below maps channels one to one and
            // silences any that are not shared. In any normal application you would implement
            // proper mixing and gain compensation.
            for (captured, rendered) in input
                .chunks(input_channels)
                .zip(output.chunks_mut(output_channels))
            {
                rendered[..shared_channels].copy_from_slice(&captured[..shared_channels]);
                rendered[shared_channels..].fill(T::EQUILIBRIUM);
            }
        },
        err_fn,
        None,
    )?;
    println!("Successfully built the stream.");

    stream.start()?;
    println!("Playing for 10 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(10));
    drop(stream);
    println!("Done!");
    Ok(())
}

fn err_fn(err: Error) {
    match err.kind() {
        ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {
            eprintln!("{err}")
        }
        _ => eprintln!("Stream error: {err}"),
    }
}

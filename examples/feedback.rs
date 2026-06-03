//! Feeds back the input stream directly into the output stream.
//!
//! Assumes that the input and output devices can use the same stream configuration and that they
//! support the f32 sample format.
use std::sync::Arc;
use std::sync::Mutex;
use clap::Parser;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Error, ErrorKind, HostId, InputCallbackInfo, OutputCallbackInfo, Sample, StreamConfig,
};
use ringbuf::{
    traits::{Consumer, Producer, Split},
    HeapRb,
};

#[derive(Parser, Debug)]
#[command(version, about = "CPAL feedback example", long_about = None)]
struct Opt {
    /// The input audio device to use
    #[arg(short, long, value_name = "IN")]
    input_device: Option<String>,

    /// The output audio device to use
    #[arg(short, long, value_name = "OUT")]
    output_device: Option<String>,

    /// Use the JACK host. Requires `--features jack`.
    #[arg(long, default_value_t = false)]
    jack: bool,

    /// Use the PulseAudio host. Requires `--features pulseaudio`.
    #[arg(long, default_value_t = false)]
    pulseaudio: bool,
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    // JACK/PulseAudio support must be enabled at compile time, and is
    // only available on some platforms.
    #[allow(unused_mut, unused_assignments)]
    let mut jack_host_id: Result<HostId, Error> = Err(ErrorKind::HostUnavailable.into());
    #[allow(unused_mut, unused_assignments)]
    let mut pulseaudio_host_id: Result<HostId, Error> = Err(ErrorKind::HostUnavailable.into());

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

        #[cfg(feature = "pulseaudio")]
        {
            pulseaudio_host_id = Ok(HostId::PulseAudio);
        }
    }

    // Manually check for flags. Can be passed through cargo with -- e.g.
    // cargo run --release --example beep --features jack -- --jack
    let host = if opt.jack {
        jack_host_id
            .and_then(cpal::host_from_id)
            .expect("make sure `--features jack` is specified, and the platform is supported")
    } else if opt.pulseaudio {
        pulseaudio_host_id
            .and_then(cpal::host_from_id)
            .expect("make sure `--features pulseaudio` is specified, and the platform is supported")
    } else {
        cpal::default_host()
    };

    // Find devices.
    let input_device = if let Some(device) = opt.input_device {
        let id = &device.parse().expect("failed to parse input device id");
        host.device_by_id(id)
    } else {
        host.default_input_device()
    }
    .expect("failed to find input device");

    let output_device = if let Some(device) = opt.output_device {
        let id = &device.parse().expect("failed to parse output device id");
        host.device_by_id(id)
    } else {
        host.default_output_device()
    }
    .expect("failed to find output device");

    println!("Using input device: \"{}\"", input_device.id()?);
    println!("Using output device: \"{}\"", output_device.id()?);

    // Using different hosts results to different configs.
    // better set it manually, if you use multiple hosts

    // We'll try and use the same configuration between streams to keep it simple.
    let config: StreamConfig = input_device.default_input_config()?.into();

    let mtx = Arc::new(Mutex::new(0));
    let mtx_2 = mtx.clone();

    // Heap access, usually slower.
    // you may use static buffers for higher performance.
    let ring = HeapRb::<f32>::new(config.sample_rate as usize);
    let (mut producer, mut consumer) = ring.split();

    let input_data_fn = move |data: &[f32], _: &InputCallbackInfo| {
        mtx.lock();
        producer.push_slice(data);
    };

    let output_data_fn = move |data: &mut [f32], _: &OutputCallbackInfo| {
        mtx_2.lock();
        consumer.pop_slice(data);
    };

    // Build streams.
    println!("Attempting to build both streams with f32 samples and `{config:?}`.");
    let input_stream = input_device.build_input_stream(config, input_data_fn, err_fn, None)?;
    let output_stream = output_device.build_output_stream(config, output_data_fn, err_fn, None)?;
    println!("Successfully built streams.");

    // Play the streams.
    println!("Starting the input and output streams");
    input_stream.play()?;
    output_stream.play()?;

    // Run for 10 seconds before closing.
    println!("Playing for 10 seconds... ");
    std::thread::sleep(std::time::Duration::from_secs(10));
    drop(input_stream);
    drop(output_stream);
    println!("Done!");
    Ok(())
}

fn err_fn(err: Error) {
    match err.kind() {
        ErrorKind::DeviceChanged | ErrorKind::Xrun | ErrorKind::RealtimeDenied => {
            eprintln!("{err}")
        }
        _ => eprintln!("Stream error: {err}"),
    }
}

//! Feeds back the input stream directly into the output stream by opening the device in full
//! duplex mode.
//!
//! Assumes that the device can use the f32 sample format.

extern crate anyhow;
extern crate clap;
extern crate cpal;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Debug)]
struct Opt {
    #[cfg(all(
        any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
        feature = "jack"
    ))]
    jack: bool,

    device: String,
}

impl Opt {
    fn from_args() -> anyhow::Result<Self> {
        let app = clap::App::new("duplex").arg_from_usage("[DEVICE] 'The audio device to use'");

        #[cfg(all(
            any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
            feature = "jack"
        ))]
        let app = app.arg_from_usage("-j, --jack 'Use the JACK host");
        let matches = app.get_matches();
        let device = matches.value_of("DEVICE").unwrap_or("default").to_string();

        #[cfg(all(
            any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
            feature = "jack"
        ))]
        return Ok(Opt {
            jack: matches.is_present("jack"),
            device,
        });

        #[cfg(any(
            not(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd")),
            not(feature = "jack")
        ))]
        Ok(Opt { device })
    }
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args()?;

    // Conditionally compile with jack if the feature is specified.
    #[cfg(all(
        any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
        feature = "jack"
    ))]
    // Manually check for flags. Can be passed through cargo with -- e.g.
    // cargo run --release --example beep --features jack -- --jack
    let host = if opt.jack {
        cpal::host_from_id(cpal::available_hosts()
            .into_iter()
            .find(|id| *id == cpal::HostId::Jack)
            .expect(
                "make sure --features jack is specified. only works on OSes where jack is available",
            )).expect("jack host unavailable")
    } else {
        cpal::default_host()
    };

    #[cfg(any(
        not(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd")),
        not(feature = "jack")
    ))]
    let host = cpal::default_host();

    // Find the device
    let device = if opt.device == "default" {
        host.default_duplex_device()
    } else {
        host.duplex_devices()?
            .find(|x| x.name().map(|y| y == opt.device).unwrap_or(false))
    }
    .expect("failed to find device");

    println!("Using device: \"{}\"", device.name()?);

    let config: cpal::DuplexStreamConfig = device.default_duplex_config()?.into();
    let input_channels: usize = config.input_channels.into();
    let output_channels: usize = config.output_channels.into();
    // Simply copy as many channels as both input and output allow
    let copy_channels = input_channels.min(output_channels);

    let data_fn = move |data_in: &[f32], data_out: &mut [f32], _: &cpal::DuplexCallbackInfo| {
        for (frame_in, frame_out) in data_in
            .chunks(input_channels)
            .zip(data_out.chunks_mut(output_channels))
        {
            frame_out[..copy_channels].clone_from_slice(&frame_in[..copy_channels]);
        }
    };

    // Build streams.
    println!(
        "Attempting to build stream with f32 samples and `{:?}`.",
        config
    );
    let stream = device.build_duplex_stream(&config, data_fn, err_fn)?;
    println!("Successfully built stream.");

    // Play the stream.
    println!("Starting the duplex stream.");
    stream.play()?;

    // Run for 3 seconds before closing.
    println!("Playing for 3 seconds... ");
    std::thread::sleep(std::time::Duration::from_secs(3));
    drop(stream);
    println!("Done!");
    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}

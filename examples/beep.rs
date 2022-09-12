extern crate anyhow;
extern crate clap;
extern crate cpal;

use std::iter;

use clap::arg;
use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, Transcoder, Endianness, samples::{self, SampleBufferMut}};
use cpal::{Sample, FromSample};

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
    fn from_args() -> Self {
        let app = clap::Command::new("beep").arg(arg!([DEVICE] "The audio device to use"));
        #[cfg(all(
            any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
            feature = "jack"
        ))]
        let app = app.arg(arg!(-j --jack "Use the JACK host"));
        let matches = app.get_matches();
        let device = matches.value_of("DEVICE").unwrap_or("default").to_string();

        #[cfg(all(
            any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
            feature = "jack"
        ))]
        return Opt {
            jack: matches.is_present("jack"),
            device,
        };

        #[cfg(any(
            not(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd")),
            not(feature = "jack")
        ))]
        Opt { device }
    }
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args();

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

    let device = if opt.device == "default" {
        host.default_output_device()
    } else {
        host.output_devices()?
            .find(|x| x.name().map(|y| y == opt.device).unwrap_or(false))
    }
    .expect("failed to find output device");
    println!("Output device: {}", device.name()?);

    let config = device.default_output_config().unwrap();
    println!("Default output config: {:?}", config);

    match config.sample_format() {
        cpal::SampleFormat::I8B1 => run::<samples::i8::B1NE>(&device, &config.into()),
        cpal::SampleFormat::I16B2(Endianness::Big) => run::<samples::i16::B2BE>(&device, &config.into()),
        cpal::SampleFormat::I16B2(Endianness::Little) => run::<samples::i16::B2LE>(&device, &config.into()),
        cpal::SampleFormat::I32B4(Endianness::Big) => run::<samples::i32::B4BE>(&device, &config.into()),
        cpal::SampleFormat::I32B4(Endianness::Little) => run::<samples::i32::B4LE>(&device, &config.into()),
        cpal::SampleFormat::I64B8(Endianness::Big) => run::<samples::i64::B8BE>(&device, &config.into()),
        cpal::SampleFormat::I64B8(Endianness::Little) => run::<samples::i64::B8LE>(&device, &config.into()),

        cpal::SampleFormat::U8B1 => run::<samples::u8::B1NE>(&device, &config.into()),
        cpal::SampleFormat::U16B2(Endianness::Big) => run::<samples::u16::B2BE>(&device, &config.into()),
        cpal::SampleFormat::U16B2(Endianness::Little) => run::<samples::u16::B2LE>(&device, &config.into()),
        cpal::SampleFormat::U32B4(Endianness::Big) => run::<samples::u32::B4BE>(&device, &config.into()),
        cpal::SampleFormat::U32B4(Endianness::Little) => run::<samples::u32::B4LE>(&device, &config.into()),
        cpal::SampleFormat::U64B8(Endianness::Big) => run::<samples::u64::B8BE>(&device, &config.into()),
        cpal::SampleFormat::U64B8(Endianness::Little) => run::<samples::u64::B8LE>(&device, &config.into()),

        cpal::SampleFormat::F32B4(Endianness::Big) => run::<samples::f32::B4BE>(&device, &config.into()),
        cpal::SampleFormat::F32B4(Endianness::Little) => run::<samples::f32::B4LE>(&device, &config.into()),
        cpal::SampleFormat::F64B8(Endianness::Big) => run::<samples::f64::B8BE>(&device, &config.into()),
        cpal::SampleFormat::F64B8(Endianness::Little) => run::<samples::f64::B8LE>(&device, &config.into()),

        sample_format => panic!("Unsupported sample format '{sample_format}'"),
    }

}

pub fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<(), anyhow::Error>
where
    T: Transcoder,
    T::Sample: FromSample<f32>,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    // Produce a sinusoid of maximum amplitude.
    let mut sample_clock = 0f32;
    let mut next_value = move || {
        sample_clock = (sample_clock + 1.0) % sample_rate;
        (sample_clock * 440.0 * 2.0 * std::f32::consts::PI / sample_rate).sin()
    };

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

    let stream = device.build_output_stream::<T, _, _>(
        config,
        move |sample_buffer: SampleBufferMut<_>, _: &cpal::OutputCallbackInfo| {
            write_data::<T>(sample_buffer, channels, &mut next_value)
        },
        err_fn,
    )?;
    stream.play()?;

    std::thread::sleep(std::time::Duration::from_millis(1000));

    Ok(())
}

fn write_data<T>(output: SampleBufferMut<T>, channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: Transcoder,
    T::Sample: FromSample<f32> ,
{
    let source = iter::from_fn(|| {
        let sample = T::Sample::from_sample(next_sample());
            Some(iter::repeat(sample).take(channels))
        }).flatten();

    output.into_iter().write_iter(source);
}

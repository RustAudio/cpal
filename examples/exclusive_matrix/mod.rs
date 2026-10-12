//! Silent, opt-in physical endpoint validation; not a hardware-rate/bit-perfect measurement.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Probe {
    stream: cpal::Stream,
    count: Arc<AtomicU64>,
    errors: mpsc::Receiver<String>,
    timestamps: mpsc::Receiver<cpal::StreamTimestamp>,
    diagnostics: mpsc::Receiver<(u64, usize, cpal::StreamTimestamp)>,
}

impl Probe {
    fn build(device: &cpal::Device, rate: u32) -> Result<Self> {
        let count = Arc::new(AtomicU64::new(0));
        let expected = Arc::new(AtomicU64::new(0));
        let (tx, errors) = mpsc::channel();
        let (timestamp_tx, timestamps) = mpsc::sync_channel(1);
        let (diagnostic_tx, diagnostics) = mpsc::sync_channel(8);
        let error_tx = tx.clone();
        let callback_count = Arc::clone(&count);
        let callback_expected = Arc::clone(&expected);
        let mut previous: Option<cpal::StreamTimestamp> = None;
        let stream = device.build_output_stream::<i32, _, _>(
            cpal::StreamConfig {
                channels: 2,
                sample_rate: rate,
                buffer_size: cpal::BufferSize::Default,
            },
            move |data, info| {
                // Never write audio: WASAPI supplies format-correct digital equilibrium.
                let timestamp = info.timestamp();
                let sequence = callback_count.load(Ordering::Relaxed) + 1;
                if sequence <= 8 {
                    let _ = diagnostic_tx.try_send((sequence, data.len() / 2, timestamp));
                }
                let zero = cpal::StreamInstant::new(0, 0);
                let mut failure = None;
                if data.len() % 2 != 0
                    || data.len() as u64 / 2 != callback_expected.load(Ordering::Relaxed)
                {
                    failure = Some("callback frame count differs from stream.buffer_size()");
                } else if data.iter().any(|&sample| sample != 0) {
                    failure = Some("backend did not initialize I32 samples to digital silence");
                } else if timestamp.callback == zero {
                    // Only the first pre-start prime may have a zero clock snapshot.
                    if previous.is_some() || timestamp.device != zero {
                        failure = Some("zero clock outside the initial pre-start prime");
                    }
                } else {
                    let _ = timestamp_tx.try_send(timestamp);
                    if timestamp.device < timestamp.callback
                        || timestamp.device.duration_since(timestamp.callback)
                            > Duration::from_secs(1)
                    {
                        failure = Some("implausible device-versus-callback timestamp");
                    }
                }
                if let Some(last) = previous {
                    if timestamp.callback < last.callback || timestamp.device < last.device {
                        failure = Some("callback/device timestamp regressed");
                    }
                }
                if let Some(message) = failure {
                    let _ = tx.send(format!("{message}; callback_count={sequence} frames={} callback_ns={} device_ns={} previous={previous:?}", data.len() / 2, timestamp.callback.as_nanos(), timestamp.device.as_nanos()));
                }
                previous = Some(timestamp);
                callback_count.fetch_add(1, Ordering::Relaxed);
            },
            move |error| {
                let _ = error_tx.send(format!("backend callback error: {error}"));
            },
            None,
        )?;
        let frames = stream.buffer_size()?;
        if frames == 0 {
            return Err("zero actual buffer frames".into());
        }
        expected.store(frames as u64, Ordering::Relaxed);
        println!("  requested stereo I32 {rate} Hz; reported callback frames={frames}");
        Ok(Self {
            stream,
            count,
            errors,
            timestamps,
            diagnostics,
        })
    }

    fn print_diagnostics(&self) {
        for (sequence, frames, timestamp) in self.diagnostics.try_iter() {
            let now = self.stream.now();
            eprintln!(
                "TIMESTAMP sample#{sequence} frames={frames} callback_ns={} device_ns={} sampled_now_ns={} callback_to_now_ns={:?} device_minus_callback_ns={:?}",
                timestamp.callback.as_nanos(),
                timestamp.device.as_nanos(),
                now.as_nanos(),
                now.checked_duration_since(timestamp.callback)
                    .map(|d| d.as_nanos()),
                timestamp
                    .device
                    .checked_duration_since(timestamp.callback)
                    .map(|d| d.as_nanos())
            );
        }
    }

    fn check(&self) -> Result<()> {
        self.print_diagnostics();
        if let Ok(error) = self.errors.try_recv() {
            return Err(format!("{error}; sampled_now_ns={}", self.stream.now().as_nanos()).into());
        }
        Ok(())
    }

    fn observe(&self) -> Result<()> {
        self.check()?;
        let before = self.count.load(Ordering::Relaxed);
        while self.timestamps.try_recv().is_ok() {}
        self.stream.start()?;
        let started = Instant::now();
        while self.count.load(Ordering::Relaxed).saturating_sub(before) < 10 {
            self.check()?;
            if started.elapsed() >= Duration::from_secs(10) {
                return Err("fewer than ten callbacks within 10s".into());
            }
            match self.errors.recv_timeout(Duration::from_millis(10)) {
                Ok(error) => {
                    self.print_diagnostics();
                    return Err(format!(
                        "{error}; sampled_now_ns={}",
                        self.stream.now().as_nanos()
                    )
                    .into());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("worker error channel disconnected".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        self.check()?;
        let timestamp = self
            .timestamps
            .try_recv()
            .map_err(|_| "No live timestamp observed")?;
        let now = self.stream.now();
        for instant in [timestamp.callback, timestamp.device] {
            if instant > now + Duration::from_secs(1)
                || now.duration_since(instant) > started.elapsed() + Duration::from_secs(1)
            {
                return Err("Callback/device timestamp not sane relative to stream.now()".into());
            }
        }
        println!(
            "  {} callbacks in {:.3}s (zero pre-start timestamps permitted)",
            self.count.load(Ordering::Relaxed) - before,
            started.elapsed().as_secs_f64()
        );
        Ok(())
    }

    fn release(self) -> Result<()> {
        let Self { stream, errors, .. } = self;
        drop(stream); // joins worker; release exclusive ownership without an explicit Reset
        if let Ok(error) = errors.try_recv() {
            return Err(error.into());
        }
        Ok(())
    }
}

pub fn run(exact_name: &str) -> Result<()> {
    let host = cpal::host_from_id(cpal::HostId::Wasapi)?;
    let mut matches = Vec::new();
    for device in host.output_devices()? {
        let name = device.description()?.name().to_owned();
        println!("Available output: {name:?}");
        if name == exact_name {
            matches.push(device);
        }
    }
    if matches.len() != 1 {
        return Err(format!(
            "Expected exactly one output named {exact_name:?}; found {}. No fallback selected.",
            matches.len()
        )
        .into());
    }
    let device = matches.remove(0);
    if !device.supports_exclusive() {
        return Err("Selected endpoint does not support exclusive output".into());
    }
    println!("Selected endpoint ID: {}", device.id()?);
    let device = device.exclusive(true);
    let configs: Vec<_> = device.supported_output_configs()?.collect();
    println!(
        "MATRIX endpoint={exact_name:?}; requested configs only, not measured DAC physical rate or bit-perfect proof"
    );
    let (mut supported, mut skipped, mut passed, mut failed) = (0, 0, 0, 0);
    for rate in [44100, 48000, 96000, 192000] {
        if !configs.iter().any(|c| {
            c.channels() == 2
                && c.sample_format() == cpal::SampleFormat::I32
                && c.contains_rate(rate)
        }) {
            skipped += 1;
            println!(
                "SKIP {rate}: stereo I32 not advertised by exclusive configured-device enumeration"
            );
            continue;
        }
        supported += 1;
        let result = (|| -> Result<()> {
            println!("RATE {rate}: first open");
            let first = Probe::build(&device, rate)?;
            first.observe()?;
            first.release()?;
            println!("RATE {rate}: reopen and contention");
            let held = Probe::build(&device, rate)?;
            held.observe()?;
            match Probe::build(&device, rate) {
                Ok(second) => {
                    second.release()?;
                    return Err("second exclusive build unexpectedly succeeded".into());
                }
                Err(error) => {
                    let Some(error) = error.downcast_ref::<cpal::Error>() else {
                        return Err(error);
                    };
                    if error.kind() != cpal::ErrorKind::DeviceBusy {
                        return Err(
                            format!("contention failed for unrelated reason: {error:?}").into()
                        );
                    }
                    println!("  contention rejected: DeviceBusy");
                }
            }
            held.observe()?;
            held.release()?;
            println!("RATE {rate}: reopen after contention/release");
            let final_open = Probe::build(&device, rate)?;
            final_open.observe()?;
            final_open.release()?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                passed += 1;
                println!("PASS {rate}");
            }
            Err(error) => {
                failed += 1;
                eprintln!("FAIL {rate}: {error}");
            }
        }
    }
    println!(
        "MATRIX SUMMARY supported/attempted={supported} passed={passed} skipped={skipped} failed={failed}"
    );
    if failed != 0 || supported == 0 {
        return Err("Matrix failed or no supported configurations were exercised".into());
    }
    println!(
        "PASS for advertised configurations only; skipped rates are not passes; no mandatory baseline rate"
    );
    Ok(())
}

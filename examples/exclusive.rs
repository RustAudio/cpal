//! Silent exclusive-output lifecycle smoke test; requires a physical/virtual audio endpoint.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
mod exclusive_matrix;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn check_errors(errors: &mpsc::Receiver<cpal::Error>) -> Result<()> {
    if let Ok(error) = errors.try_recv() {
        return Err(error.into());
    }
    Ok(())
}

fn wait_check_errors(errors: &mpsc::Receiver<cpal::Error>, duration: Duration) -> Result<()> {
    match errors.recv_timeout(duration) {
        Ok(error) => Err(error.into()),
        Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("Stream error channel closed while awaiting callbacks".into())
        }
    }
}

fn run_stage(
    stream: &cpal::Stream,
    callbacks: &AtomicU64,
    errors: &mpsc::Receiver<cpal::Error>,
    label: &str,
) -> Result<()> {
    check_errors(errors)?;
    let before = callbacks.load(Ordering::Relaxed);
    let requested = Instant::now();
    stream.start()?;
    println!("{label}: async start requested; waiting up to 10s for callback readiness");
    let deadline = requested + Duration::from_secs(10);
    while callbacks.load(Ordering::Relaxed) == before {
        check_errors(errors)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("{label}: no callback readiness within ten seconds").into());
        }
        wait_check_errors(errors, remaining.min(Duration::from_millis(10)))?;
    }
    check_errors(errors)?;
    let ready = callbacks.load(Ordering::Relaxed);
    println!(
        "{label}: callback ready after {:.3}s; observing another 1s",
        requested.elapsed().as_secs_f64()
    );
    let observation_end = Instant::now() + Duration::from_secs(1);
    loop {
        let remaining = observation_end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        wait_check_errors(errors, remaining.min(Duration::from_millis(10)))?;
    }
    check_errors(errors)?;
    let observed = callbacks.load(Ordering::Relaxed).saturating_sub(ready);
    println!("{label}: {observed} additional callbacks observed");
    if observed == 0 {
        return Err(
            format!("{label}: callback progress stopped during one-second observation").into(),
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        #[cfg(target_os = "windows")]
        if args.first().map(String::as_str) == Some("--matrix") {
            let name = match args.as_slice() {
                [_, flag, name] if flag == "--device" => name.as_str(),
                [_] => return Err(
                    "Usage: exclusive --matrix --device EXACT_NAME (the device name is required)"
                        .into(),
                ),
                _ => return Err("Usage: exclusive --matrix --device EXACT_NAME".into()),
            };
            return exclusive_matrix::run(name);
        }
        return Err("Unsupported arguments for this platform".into());
    }
    println!("Silent exclusive-output lifecycle smoke test (not a bit-perfect test)");
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No output device; an audio endpoint is required")?;
    if !device.supports_exclusive() {
        return Err("This device/backend does not implement exclusive output".into());
    }
    let device = device.exclusive(true);
    let supported = device.default_output_config()?;
    println!("{}: {:?}", device, supported);
    let callbacks = Arc::new(AtomicU64::new(0));
    let callback_count = Arc::clone(&callbacks);
    let (error_tx, errors) = mpsc::channel();
    let stream = device.build_output_stream_raw(
        supported.config(),
        supported.sample_format(),
        move |_, _| {
            callback_count.fetch_add(1, Ordering::Relaxed);
        },
        move |error| {
            let _ = error_tx.send(error);
        },
        None,
    )?;
    println!("Actual callback frames: {}", stream.buffer_size()?);
    run_stage(&stream, &callbacks, &errors, "Initial start")?;
    stream.pause()?;
    run_stage(&stream, &callbacks, &errors, "Resume after pause request")?;
    stream.stop(Some(Duration::from_secs(1)))?;
    run_stage(
        &stream,
        &callbacks,
        &errors,
        "Restart after bounded stop request",
    )?;
    stream.stop(Some(Duration::ZERO))?;
    drop(stream);
    check_errors(&errors)?;
    println!("PASS: callback progress in all three running stages; no callback errors");
    Ok(())
}

use std::{cell::Cell, rc::Rc};

use cpal::{
    Device, DuplexCallbackInfo, Error, ErrorKind, FromSample, HostId, Sample, SampleFormat,
    SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Producer, Split},
};
use wasm_bindgen::prelude::*;
use web_sys::console;

// This is like the `main` function, except for JavaScript.
#[wasm_bindgen(start)]
pub fn main_js() -> Result<(), JsValue> {
    // This provides better error messages in debug mode.
    // It's disabled in release mode, so it doesn't bloat up the file size.
    #[cfg(debug_assertions)]
    console_error_panic_hook::set_once();

    let document = gloo::utils::document();
    let play_button = document.get_element_by_id("play").unwrap();
    let stop_button = document.get_element_by_id("stop").unwrap();
    let record_button = document.get_element_by_id("record").unwrap();
    let stop_record_button = document.get_element_by_id("stop-record").unwrap();
    let duplex_button = document.get_element_by_id("duplex").unwrap();
    let stop_duplex_button = document.get_element_by_id("stop-duplex").unwrap();

    // stream needs to be referenced from the "play" and "stop" closures
    let stream = Rc::new(Cell::new(None));

    // set up play button
    {
        let stream = stream.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::MouseEvent| {
            stream.set(Some(beep()));
        });
        play_button
            .add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // set up stop button
    {
        let closure = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::MouseEvent| {
            // stop the stream by dropping it
            stream.take();
        });
        stop_button
            .add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // input stream needs its own slot; recording and playback run independently
    let record_stream = Rc::new(Cell::new(None));

    // set up record button
    {
        let record_stream = record_stream.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::MouseEvent| {
            record_stream.set(Some(record()));
        });
        record_button
            .add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // set up stop-record button
    {
        let closure = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::MouseEvent| {
            // stop the stream by dropping it; releases the microphone
            record_stream.take();
        });
        stop_record_button
            .add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // duplex loopback is a single stream, so one slot holds the whole thing
    let duplex_stream = Rc::new(Cell::new(None));

    // set up duplex button
    {
        let duplex_stream = duplex_stream.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::MouseEvent| {
            duplex_stream.set(Some(duplex()));
        });
        duplex_button
            .add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // set up stop-duplex button
    {
        let closure = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::MouseEvent| {
            // stop the stream by dropping it; releases the microphone
            duplex_stream.take();
        });
        stop_duplex_button
            .add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    Ok(())
}

fn beep() -> Stream {
    let host = cpal::host_from_id(HostId::AudioWorklet).expect("AudioWorklet host not available");

    let device = host
        .default_output_device()
        .expect("failed to find a default output device");
    let config = device.default_output_config().unwrap();

    match config.sample_format() {
        SampleFormat::F32 => run::<f32>(&device, config.into()),
        SampleFormat::I16 => run::<i16>(&device, config.into()),
        SampleFormat::U16 => run::<u16>(&device, config.into()),
        _ => panic!("unsupported sample format"),
    }
}

fn run<T>(device: &Device, config: StreamConfig) -> Stream
where
    T: Sample + SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate as f32;
    let channels = config.channels as usize;

    // Produce a sinusoid of maximum amplitude.
    let mut sample_clock = 0f32;
    let mut next_value = move || {
        sample_clock = (sample_clock + 1.0) % sample_rate;
        (sample_clock * 440.0 * 2.0 * std::f32::consts::PI / sample_rate).sin()
    };

    let err_fn = |err: Error| match err.kind() {
        ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {
            console::log_1(&format!("{err}").into())
        }
        _ => console::error_1(&format!("Stream error: {err}").into()),
    };

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| write_data(data, channels, &mut next_value),
            err_fn,
            None,
        )
        .unwrap();
    stream.start().unwrap();
    stream
}

/// Captures microphone input into a ring buffer and immediately plays it back, so you can hear
/// your own voice. Wear headphones: routing a live mic to speakers risks feedback howl.
fn record() -> (Stream, Stream) {
    let host = cpal::host_from_id(HostId::AudioWorklet).expect("AudioWorklet host not available");

    let input_device = host
        .default_input_device()
        .expect("failed to find a default input device");
    let output_device = host
        .default_output_device()
        .expect("failed to find a default output device");

    let input_config = input_device.default_input_config().unwrap();
    let output_config = output_device.default_output_config().unwrap();

    // Bound end-to-end latency; once full, the producer drops the newest samples instead of
    // blocking.
    let max_buffered_samples =
        output_config.sample_rate() as usize * output_config.channels() as usize / 2;
    let ring = HeapRb::<f32>::new(max_buffered_samples);
    let (producer, consumer) = ring.split();

    let input_stream = match input_config.sample_format() {
        SampleFormat::F32 => build_input::<f32>(&input_device, input_config.into(), producer),
        SampleFormat::I16 => build_input::<i16>(&input_device, input_config.into(), producer),
        SampleFormat::U16 => build_input::<u16>(&input_device, input_config.into(), producer),
        _ => panic!("unsupported sample format"),
    };
    let output_stream = match output_config.sample_format() {
        SampleFormat::F32 => build_output::<f32>(&output_device, output_config.into(), consumer),
        SampleFormat::I16 => build_output::<i16>(&output_device, output_config.into(), consumer),
        SampleFormat::U16 => build_output::<u16>(&output_device, output_config.into(), consumer),
        _ => panic!("unsupported sample format"),
    };

    (input_stream, output_stream)
}

/// The same live microphone loopback as [`record`], but as one duplex stream instead of two
/// independent ones. `AudioWorkletProcessor.process(inputs, outputs)` hands both directions to a
/// single callback on a single clock, so no ring buffer is needed to bridge them and the round
/// trip is a callback rather than a delay line. Wear headphones: routing a live mic to speakers
/// risks feedback howl.
fn duplex() -> Stream {
    let host = cpal::host_from_id(HostId::AudioWorklet).expect("AudioWorklet host not available");

    let device = host
        .default_output_device()
        .expect("failed to find a default output device");
    assert!(
        device.supports_duplex(),
        "duplex streams need `navigator.mediaDevices`, which browsers expose only in a \
         secure context: serve this page over HTTPS or from localhost"
    );

    let config = device.default_duplex_config().unwrap();

    let err_fn = |err: Error| match err.kind() {
        ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {
            console::log_1(&format!("{err}").into())
        }
        _ => console::error_1(&format!("Stream error: {err}").into()),
    };

    // WebAudio processes exclusively in f32, so there is no sample format to match on here.
    let input_channels = config.input_channels as usize;
    let output_channels = config.output_channels as usize;
    let stream = device
        .build_duplex_stream(
            config,
            move |input: &[f32], output: &mut [f32], _: &DuplexCallbackInfo| {
                // The two directions can carry different channel counts, so mix each captured
                // frame down to mono and fan it back out across the output frame.
                for (captured, rendered) in input
                    .chunks(input_channels)
                    .zip(output.chunks_mut(output_channels))
                {
                    rendered.fill(captured.iter().sum::<f32>() / input_channels as f32);
                }
            },
            err_fn,
            None,
        )
        .unwrap();
    stream.start().unwrap();
    stream
}

fn build_input<T>(device: &Device, config: StreamConfig, mut producer: HeapProd<f32>) -> Stream
where
    T: Sample + SizedSample,
    f32: FromSample<T>,
{
    let err_fn = |err: Error| match err.kind() {
        ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {
            console::log_1(&format!("{err}").into())
        }
        _ => console::error_1(&format!("Stream error: {err}").into()),
    };

    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                producer.push_iter(data.iter().map(|&s| f32::from_sample(s)));
            },
            err_fn,
            None,
        )
        .unwrap();
    stream.start().unwrap();
    stream
}

fn build_output<T>(device: &Device, config: StreamConfig, mut consumer: HeapCons<f32>) -> Stream
where
    T: Sample + SizedSample + FromSample<f32>,
{
    let err_fn = |err: Error| match err.kind() {
        ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {
            console::log_1(&format!("{err}").into())
        }
        _ => console::error_1(&format!("Stream error: {err}").into()),
    };

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for sample in data.iter_mut() {
                    let value = consumer.try_pop().unwrap_or(f32::EQUILIBRIUM);
                    *sample = T::from_sample(value);
                }
            },
            err_fn,
            None,
        )
        .unwrap();
    stream.start().unwrap();
    stream
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let sample = next_sample();
        let value = T::from_sample::<f32>(sample);
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

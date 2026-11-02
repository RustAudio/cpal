//! Building a WebAudio stream that fails after its `AudioContext` exists must close that context.

#![cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen"
))]

use cpal::{
    BufferSize, DuplexStreamConfig, ErrorKind, StreamConfig,
    traits::{DeviceTrait, HostTrait},
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen(inline_js = r#"
export function watchClose() {
    const close = AudioContext.prototype.close;
    let calls = 0;
    AudioContext.prototype.close = function (...args) {
        calls += 1;
        return close.apply(this, args);
    };
    return () => {
        AudioContext.prototype.close = close;
        return calls;
    };
}

export function destinationMaxChannels() {
    const ctx = new AudioContext();
    const max = ctx.destination.maxChannelCount;
    ctx.close();
    return max;
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = watchClose)]
    fn watch_close() -> js_sys::Function;
    #[wasm_bindgen(js_name = destinationMaxChannels)]
    fn destination_max_channels() -> u16;
}

/// One channel more than the destination takes, if that is still a channel count the host
/// accepts (so the build gets as far as creating the context).
fn too_many_channels() -> Option<u16> {
    let channels = destination_max_channels() + 1;
    (channels <= 32).then_some(channels)
}

/// Run `build`, which is expected to fail on the destination's channel count, and return how
/// many `AudioContext`s were closed meanwhile.
fn closes_during(build: impl FnOnce() -> Result<cpal::Stream, cpal::Error>) -> u32 {
    let stop_watching = watch_close();
    let result = build();
    let closes = stop_watching
        .call0(&JsValue::NULL)
        .unwrap()
        .as_f64()
        .unwrap() as u32;
    let err = result.err().expect("the build should fail");
    assert_eq!(err.kind(), ErrorKind::UnsupportedConfig, "{err}");
    closes
}

#[wasm_bindgen_test]
fn output_stream_with_too_many_channels_closes_its_context() {
    let Some(channels) = too_many_channels() else {
        return;
    };
    let device = cpal::default_host().default_output_device().unwrap();
    let config = StreamConfig {
        channels,
        sample_rate: 48_000,
        buffer_size: BufferSize::Default,
    };
    let closes = closes_during(|| {
        device.build_output_stream(config, |_: &mut [f32], _: &_| {}, |_| {}, None)
    });
    assert_eq!(closes, 1);
}

#[wasm_bindgen_test]
fn duplex_stream_with_too_many_output_channels_closes_its_context() {
    let Some(channels) = too_many_channels() else {
        return;
    };
    let device = cpal::default_host().default_output_device().unwrap();
    let config = DuplexStreamConfig {
        input_channels: 1,
        output_channels: channels,
        sample_rate: 48_000,
        buffer_size: BufferSize::Default,
    };
    let closes = closes_during(|| {
        device.build_duplex_stream(config, |_: &[f32], _: &mut [f32], _: &_| {}, |_| {}, None)
    });
    assert_eq!(closes, 1);
}

#![cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen"
))]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use cpal::{
    BufferSize, HostId, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use futures_channel::oneshot;
use futures_util::future::{Either, select};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen(inline_js = r#"
export function watchAudio() {
    const errors = [];
    const sources = [];
    const hadOwnMethod = Object.hasOwn(AudioContext.prototype, 'createBufferSource');
    const createBufferSource = AudioContext.prototype.createBufferSource;
    AudioContext.prototype.createBufferSource = function (...args) {
        const source = createBufferSource.apply(this, args);
        sources.push(source);
        return source;
    };
    const onError = event => errors.push(String(event.message || event.reason));
    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onError);
    return () => {
        if (hadOwnMethod) AudioContext.prototype.createBufferSource = createBufferSource;
        else delete AudioContext.prototype.createBufferSource;
        window.removeEventListener('error', onError);
        window.removeEventListener('unhandledrejection', onError);
        return [errors.join('\n'), sources.filter(source => source.onended !== null).length, sources.length];
    };
}

export function delay(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

export function settle() {
    return Promise.resolve();
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = watchAudio)]
    fn watch_audio() -> js_sys::Function;
    fn delay(ms: i32) -> js_sys::Promise;
    fn settle() -> js_sys::Promise;
}

struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

async fn check_drop(start: bool, callbacks_before_drop: usize) {
    let stop_watching = watch_audio();
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(AtomicUsize::new(0));
    let data_probe = DropProbe(drops.clone());
    let error_probe = DropProbe(drops.clone());
    let calls_callback = calls.clone();
    let errors_callback = errors.clone();
    let (ready_tx, ready_rx) = oneshot::channel();
    let mut ready_tx = Some(ready_tx);
    let host = cpal::host_from_id(HostId::WebAudio).unwrap();
    let device = host.default_output_device().unwrap();
    let stream = device
        .build_output_stream::<f32, _, _>(
            StreamConfig {
                channels: 1,
                sample_rate: 48_000,
                buffer_size: BufferSize::Fixed(4096),
            },
            move |data, _| {
                let _ = &data_probe;
                let count = calls_callback.fetch_add(1, Ordering::Relaxed) + 1;
                data.fill(0.0);
                if count >= callbacks_before_drop {
                    if let Some(tx) = ready_tx.take() {
                        let _ = tx.send(());
                    }
                }
            },
            move |_| {
                let _ = &error_probe;
                errors_callback.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )
        .unwrap();

    if start {
        stream.start().unwrap();
        // On +atomics, startup is queued on the local task. Let it schedule its timers.
        wasm_bindgen_futures::JsFuture::from(settle())
            .await
            .unwrap();
    }
    if callbacks_before_drop > 0 {
        // Resume in a microtask after the callback returns, before the next timer/event.
        match select(ready_rx, wasm_bindgen_futures::JsFuture::from(delay(2000))).await {
            Either::Left((Ok(()), _)) => {}
            _ => panic!("audio callbacks did not arrive"),
        }
    }
    let calls_before_drop = calls.load(Ordering::Relaxed);
    assert!(calls_before_drop >= callbacks_before_drop);
    if callbacks_before_drop < 2 {
        assert_eq!(calls_before_drop, callbacks_before_drop);
    }
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(stream);
    // +atomics releases JS resources when the command receiver observes closure.
    wasm_bindgen_futures::JsFuture::from(settle())
        .await
        .unwrap();
    let drops_after_drop = drops.load(Ordering::Relaxed);
    wasm_bindgen_futures::JsFuture::from(delay(300))
        .await
        .unwrap();
    let report = js_sys::Array::from(&stop_watching.call0(&JsValue::UNDEFINED).unwrap());
    let js_errors = report.get(0).as_string().unwrap();
    let retained_listeners = report.get(1).as_f64().unwrap();
    let observed_sources = report.get(2).as_f64().unwrap();
    console_log!(
        "start={start}, requested callbacks={callbacks_before_drop}, calls before/after drop={}/{}, probes dropped={drops_after_drop}, stream errors={}, sources={observed_sources}, retained listeners={retained_listeners}, JS errors={js_errors:?}",
        calls_before_drop,
        calls.load(Ordering::Relaxed),
        errors.load(Ordering::Relaxed),
    );
    assert_eq!(drops_after_drop, 2, "callbacks retained after stream drop");
    assert_eq!(
        calls.load(Ordering::Relaxed),
        calls_before_drop,
        "late data callback"
    );
    assert_eq!(errors.load(Ordering::Relaxed), 0, "stream error callback");
    assert_eq!(js_errors, "", "late JavaScript error");
    assert!(observed_sources >= callbacks_before_drop as f64);
    assert_eq!(
        retained_listeners, 0.0,
        "source retained its ended listener"
    );
}

#[wasm_bindgen_test(async)]
async fn drop_without_start_releases_callbacks() {
    check_drop(false, 0).await;
}

#[wasm_bindgen_test(async)]
async fn drop_before_startup_timers_releases_callbacks() {
    check_drop(true, 0).await;
}

#[wasm_bindgen_test(async)]
async fn drop_between_startup_timers_releases_callbacks() {
    check_drop(true, 1).await;
}

#[wasm_bindgen_test(async)]
async fn drop_after_rescheduling_releases_callbacks() {
    check_drop(true, 6).await;
}

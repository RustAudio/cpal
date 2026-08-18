#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "windows",
    target_vendor = "apple",
    target_os = "android",
))]
pub(crate) mod equilibrium;

#[cfg(windows)]
pub(crate) mod com;

#[cfg(target_os = "android")]
pub(crate) mod aaudio;

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
))]
pub(crate) mod alsa;

#[cfg(all(windows, feature = "asio"))]
pub(crate) mod asio;

#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen",
    feature = "audioworklet",
    target_feature = "atomics"
))]
pub(crate) mod audioworklet;

#[cfg(target_vendor = "apple")]
pub(crate) mod coreaudio;

#[cfg(all(
    feature = "jack",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "macos",
        target_os = "windows",
    )
))]
pub(crate) mod jack;

#[cfg(all(
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
    ),
    feature = "pipewire"
))]
pub(crate) mod pipewire;

#[cfg(all(
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ),
    feature = "pulseaudio"
))]
pub(crate) mod pulseaudio;

#[cfg(windows)]
pub(crate) mod wasapi;

#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen"
))]
pub(crate) mod webaudio;

#[cfg(feature = "custom")]
pub(crate) mod custom;

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_vendor = "apple",
    target_os = "android",
    all(
        target_arch = "wasm32",
        target_os = "unknown",
        feature = "wasm-bindgen"
    ),
)))]
pub(crate) mod null;

#[cfg(any(
    target_vendor = "apple",
    target_os = "windows",
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    all(
        feature = "jack",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows",
        )
    ),
))]
pub(crate) mod latch;

/// Mutex-guarded bool with Condvar for cross-thread signaling.
///
/// The bool tracks whether the notified side has acknowledged the signal.
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
))]
pub(crate) type Notify = (std::sync::Mutex<bool>, std::sync::Condvar);

/// Shared error-callback type that hands the callback across thread boundaries.
#[allow(dead_code)]
pub(crate) type ErrorCallbackArc = std::sync::Arc<std::sync::Mutex<dyn FnMut(crate::Error) + Send>>;

/// Error-delivery helpers shared by backends that hold an `ErrorCallbackArc`.
#[cfg(any(
    target_os = "android",
    target_vendor = "apple",
    target_os = "windows",
    all(
        feature = "jack",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows",
        )
    ),
    all(
        feature = "pipewire",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
        )
    ),
    all(
        feature = "pulseaudio",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
        )
    ),
))]
pub(crate) mod error_emit;

#[cfg(any(
    target_os = "android",
    target_vendor = "apple",
    target_os = "windows",
    all(
        feature = "jack",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows",
        )
    ),
    all(
        feature = "pipewire",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
        )
    ),
    all(
        feature = "pulseaudio",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
        )
    ),
))]
pub(crate) use error_emit::emit_error;

#[cfg(any(
    target_vendor = "apple",
    target_os = "android",
    all(
        feature = "jack",
        feature = "realtime",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "macos",
            target_os = "windows",
        )
    ),
    all(
        feature = "pipewire",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
        )
    ),
))]
pub(crate) use error_emit::try_emit_error;

/// Convert a frame count at a given sample rate to a [`std::time::Duration`].
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "windows",
    target_vendor = "apple",
    feature = "audioworklet",
))]
#[inline]
pub(crate) fn frames_to_duration(
    frames: crate::FrameCount,
    rate: crate::SampleRate,
) -> std::time::Duration {
    if rate == 0 {
        return std::time::Duration::ZERO;
    }
    let rate = rate as u64;
    let secs = frames as u64 / rate;
    // rem_frames < rate <= u32::MAX, so rem_frames * 1_000_000_000 < u64::MAX
    let rem_frames = frames as u64 % rate;
    // Round to nearest so the duration isn't biased.
    let nanos = (rem_frames * 1_000_000_000 + rate / 2) / rate;
    std::time::Duration::new(secs, nanos as u32)
}

/// Clamps a timestamp so it never precedes one we've already returned.
#[allow(dead_code)]
fn non_decreasing(floor: &mut u64, instant: crate::StreamInstant) -> crate::StreamInstant {
    // u64 nanos covers ~585 years of runtime.
    let nanos = instant.as_nanos().min(u64::MAX as u128) as u64;
    *floor = (*floor).max(nanos);
    crate::StreamInstant::from_nanos(*floor)
}

/// Wraps an input data callback so the `capture` timestamp never regresses across callbacks.
#[allow(dead_code)]
pub(crate) fn monotonic_input_callback<D>(
    mut data_callback: D,
) -> impl FnMut(&crate::Data, &crate::CallbackInfo) + Send + 'static
where
    D: FnMut(&crate::Data, &crate::CallbackInfo) + Send + 'static,
{
    // FnMut runs on one thread at a time, so the floor needs no synchronization.
    let mut floor = 0u64;
    move |data, info| {
        let mut info = *info;
        info.timestamp.device = non_decreasing(&mut floor, info.timestamp.device);
        data_callback(data, &info);
    }
}

/// Wraps an output data callback so the `playback` timestamp never regresses across callbacks.
#[allow(dead_code)]
pub(crate) fn monotonic_output_callback<D>(
    mut data_callback: D,
) -> impl FnMut(&mut crate::Data, &crate::CallbackInfo) + Send + 'static
where
    D: FnMut(&mut crate::Data, &crate::CallbackInfo) + Send + 'static,
{
    let mut floor = 0u64;
    move |data, info| {
        let mut info = *info;
        info.timestamp.device = non_decreasing(&mut floor, info.timestamp.device);
        data_callback(data, &info);
    }
}

/// Wraps a duplex data callback so neither direction's `device` timestamp regresses across
/// callbacks. The two directions are clamped independently.
#[allow(dead_code)]
pub(crate) fn monotonic_duplex_callback<D>(
    mut data_callback: D,
) -> impl FnMut(&crate::Data, &mut crate::Data, &crate::DuplexCallbackInfo) + Send + 'static
where
    D: FnMut(&crate::Data, &mut crate::Data, &crate::DuplexCallbackInfo) + Send + 'static,
{
    let mut input_floor = 0u64;
    let mut output_floor = 0u64;
    move |input, output, info| {
        let mut info = *info;
        info.input.timestamp.device = non_decreasing(&mut input_floor, info.input.timestamp.device);
        info.output.timestamp.device =
            non_decreasing(&mut output_floor, info.output.timestamp.device);
        data_callback(input, output, &info);
    }
}

/// Maps a rejected `getUserMedia()` promise to a [`crate::Error`], based on the DOMException
/// `name` the browser rejects with.
///
/// <https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia#exceptions>
#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen"
))]
pub(crate) fn get_user_media_error(js_err: &wasm_bindgen::JsValue) -> crate::Error {
    use crate::{Error, ErrorKind};

    let name = js_sys::Reflect::get(js_err, &wasm_bindgen::JsValue::from_str("name"))
        .ok()
        .and_then(|v| v.as_string());
    let message = js_sys::Reflect::get(js_err, &wasm_bindgen::JsValue::from_str("message"))
        .ok()
        .and_then(|v| v.as_string())
        .filter(|s| !s.is_empty())
        .or_else(|| name.clone())
        .unwrap_or_else(|| "unknown error".to_string());

    let kind = match name.as_deref() {
        Some("NotAllowedError" | "SecurityError") => ErrorKind::PermissionDenied,
        Some("NotFoundError") => ErrorKind::DeviceNotAvailable,
        Some("OverconstrainedError") => ErrorKind::UnsupportedConfig,
        Some("NotReadableError") => ErrorKind::DeviceBusy,
        Some("TypeError") => ErrorKind::InvalidInput,
        _ => ErrorKind::BackendError,
    };

    Error::with_message(kind, format!("getUserMedia() failed: {message}"))
}

/// Requests microphone access via `getUserMedia()`.
///
/// <https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia>
#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen"
))]
pub(crate) async fn request_microphone() -> Result<web_sys::MediaStream, wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;

    let constraints = web_sys::MediaStreamConstraints::new();
    constraints.set_audio_bool(true);

    let window =
        web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("No window available"))?;
    let media_devices = window.navigator().media_devices()?;
    let promise = media_devices.get_user_media_with_constraints(&constraints)?;
    let media_stream = wasm_bindgen_futures::JsFuture::from(promise).await?;
    Ok(media_stream.unchecked_into::<web_sys::MediaStream>())
}

/// Whether the current context can request microphone access. There is no way to know if a
/// microphone is actually present without asking for permission first via getUserMedia().
#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen"
))]
pub(crate) fn is_get_user_media_available() -> bool {
    web_sys::window().is_some_and(|w| w.navigator().media_devices().is_ok())
}

/// Stops every audio track of `media_stream`, releasing the microphone and turning off the
/// browser's capture indicator. Dropping a WebAudio graph alone does not do this.
#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen"
))]
pub(crate) fn stop_tracks(media_stream: &web_sys::MediaStream) {
    use wasm_bindgen::JsCast;

    for track in media_stream.get_audio_tracks().iter() {
        if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
            track.stop();
        }
    }
}

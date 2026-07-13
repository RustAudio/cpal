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

// Unlike `emit_error`, PulseAudio has no RT callback path and never calls this.
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
))]
pub(crate) use error_emit::try_emit_error;

/// Convert a frame count at a given sample rate to a [`std::time::Duration`].
#[cfg(any(
    target_os = "linux",
    target_os = "windows",
    target_vendor = "apple",
    feature = "audioworklet",
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
    )
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
) -> impl FnMut(&crate::Data, &crate::InputCallbackInfo) + Send + 'static
where
    D: FnMut(&crate::Data, &crate::InputCallbackInfo) + Send + 'static,
{
    // FnMut runs on one thread at a time, so the floor needs no synchronization.
    let mut floor = 0u64;
    move |data, info| {
        let mut info = *info;
        info.timestamp.capture = non_decreasing(&mut floor, info.timestamp.capture);
        data_callback(data, &info);
    }
}

/// Wraps an output data callback so the `playback` timestamp never regresses across callbacks.
#[allow(dead_code)]
pub(crate) fn monotonic_output_callback<D>(
    mut data_callback: D,
) -> impl FnMut(&mut crate::Data, &crate::OutputCallbackInfo) + Send + 'static
where
    D: FnMut(&mut crate::Data, &crate::OutputCallbackInfo) + Send + 'static,
{
    let mut floor = 0u64;
    move |data, info| {
        let mut info = *info;
        info.timestamp.playback = non_decreasing(&mut floor, info.timestamp.playback);
        data_callback(data, &info);
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "windows",
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

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
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
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
)))]
pub(crate) mod null;

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
    let nanos = rem_frames * 1_000_000_000 / rate;
    std::time::Duration::new(secs, nanos as u32)
}

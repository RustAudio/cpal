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
use crate::{FrameCount, SampleRate};

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
use std::time::Duration;

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

/// Deliver an error that the app must not miss, blocking if the callback is currently
/// executing on another thread. Use this for fatal or actionable errors.
#[cfg(any(
    target_vendor = "apple",
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
pub(crate) fn emit_error<E>(callback: &std::sync::Arc<std::sync::Mutex<E>>, error: crate::Error)
where
    E: FnMut(crate::Error) + Send + ?Sized,
{
    let mut cb = callback.lock().unwrap_or_else(|e| e.into_inner());
    cb(error);
}

/// Try to deliver an error without blocking the caller.
///
/// Silently drops the error if the callback is currently executing on another thread.
/// Use this only for non-fatal notifications where missing one occurrence is acceptable
/// and blocking a real-time thread is not.
#[cfg(any(
    target_vendor = "apple",
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
pub(crate) fn try_emit_error<E>(callback: &std::sync::Arc<std::sync::Mutex<E>>, error: crate::Error)
where
    E: FnMut(crate::Error) + Send + ?Sized,
{
    match callback.try_lock() {
        Ok(mut cb) => cb(error),
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner()(error),
        Err(std::sync::TryLockError::WouldBlock) => {}
    }
}

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
pub(crate) fn frames_to_duration(frames: FrameCount, rate: SampleRate) -> Duration {
    if rate == 0 {
        return Duration::ZERO;
    }
    let rate = rate as u64;
    let secs = frames as u64 / rate;
    // rem_frames < rate <= u32::MAX, so rem_frames * 1_000_000_000 < u64::MAX
    let rem_frames = frames as u64 % rate;
    let nanos = rem_frames * 1_000_000_000 / rate;
    Duration::new(secs, nanos as u32)
}

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
    feature = "wasm-bindgen",
    feature = "audioworklet",
    target_feature = "atomics"
))]
pub(crate) mod audioworklet;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod coreaudio;
#[cfg(target_os = "emscripten")]
pub(crate) mod emscripten;
#[cfg(all(
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ),
    feature = "jack"
))]
pub(crate) mod jack;
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
    target_os = "macos",
    target_os = "ios",
    target_os = "emscripten",
    target_os = "android",
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
)))]
pub(crate) mod null;

/// Compile-time assertion that a type implements Send.
/// Use this macro in each host module to ensure Stream is Send.
#[macro_export]
macro_rules! assert_stream_send {
    ($t:ty) => {
        const fn _assert_stream_send<T: Send>() {}
        const _: () = _assert_stream_send::<$t>();
    };
}

/// Compile-time assertion that a type implements Sync.
/// Use this macro in each host module to ensure Stream is Sync.
#[macro_export]
macro_rules! assert_stream_sync {
    ($t:ty) => {
        const fn _assert_stream_sync<T: Sync>() {}
        const _: () = _assert_stream_sync::<$t>();
    };
}

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
pub(crate) mod null;

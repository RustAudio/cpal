#[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"))]
pub(crate) mod alsa;
#[cfg(all(windows, feature = "asio"))]
pub(crate) mod asio;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod coreaudio;
#[cfg(target_os = "emscripten")]
pub(crate) mod emscripten;
#[cfg(all(
    any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
    feature = "jack"
))]
pub(crate) mod jack;
pub(crate) mod null;
#[cfg(target_os = "android")]
pub(crate) mod oboe;
#[cfg(any(
    target_os = "openbsd",
    all(target_os = "linux", feature = "linux-sndio")
))]
pub(crate) mod sndio;
#[cfg(windows)]
pub(crate) mod wasapi;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub(crate) mod webaudio;

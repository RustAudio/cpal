pub use self::platform::*;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[path = "alsa/mod.rs"]
mod platform;

#[cfg(windows)]
#[path = "windows/mod.rs"]
mod platform;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[path = "coreaudio/mod.rs"]
mod platform;

#[cfg(target_os = "emscripten")]
#[path = "emscripten/mod.rs"]
mod platform;

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Added `Driver::latencies()`
- `asio_message` now dispatches `kAsioResyncRequest` and `kAsioLatenciesChanged` to callbacks
  instead of silently ignoring them
- `sample_rate_did_change` now dispatches `AsioDriverEvent::SampleRateChanged` to registered
  callbacks when the reported rate differs from the last known rate

### Changed
- `Driver::add_message_callback` and `Driver::remove_message_callback` replaced by
  `Driver::add_event_callback` and `Driver::remove_event_callback`
- `MessageCallback` renamed to `DriverEventCallback`, and `MessageCallbackId` renamed to
  `DriverEventCallbackId`. `DriverEventCallback` wraps `Fn(AsioDriverEvent) -> bool` where
  `AsioDriverEvent` is a new enum covering both `asioMessage` selector events and
  `sampleRateDidChange` notifications
- `CallbackId` renamed to `BufferCallbackId`
- Public-facing `c_long` fields and return types replaced with `i32`
- Public-facing `c_double` parameters and return types replaced with `f64`
- `Driver::latencies()` now returns `Latencies { input, output }`
- `Driver::buffersize_range()` now returns `BufferSizeRange { min, max }`
- `CallbackInfo::system_time` is now `u64` nanoseconds
- `AsioError::ASE_NoMemory` renamed to `AsioError::NoMemory`
- `AsioTime::reserved`, `AsioTimeInfo::reserved`, `AsioTimeCode::future` fields made private.
- `asio_import` module is now `pub(crate)`; raw bindgen types are no longer public API
- `asio_message` delegates `kAsioSelectorSupported` for unknown selectors to registered
  callbacks, so each host decides which capabilities it opts into

### Fixed
- `Asio::load_driver` now returns `LoadDriverError::LoadDriverFailed` instead of panicking when the
  driver name contains a null byte
- Fixed TOCTOU race condition when creating streams concurrently
- `Driver::set_sample_rate` now performs a dummy buffer cycle and driver reload when
  the driver does not apply the rate change immediately, as required by some drivers
  (e.g. Steinberg)
- Fixed `asio_message` not advertising `kAsioSelectorSupported` itself as a supported selector
- Fixed data race where `channels`, `latencies`, `sample_rate`, and related query methods could
  call ASIO concurrently during `set_sample_rate`'s teardown/reload
- Fix rust-analyzer errors on non-Windows targets by using stub instead of ASIO bindings

### Removed
- Removed unused `SampleRate` struct
- `DriverState` is no longer part of the public API

## [0.2.6] - 2026-02-18

### Fixed
- Link `advapi32` to resolve Windows Registry API symbols

## [0.2.5] - 2026-01-04

### Fixed
- Fixed ASIO SDK discovery on case sensitive filesystems

## [0.2.4] - 2025-12-20

### Fixed
- Fixed docs.rs documentation build by generating stub bindings when building for docs.rs
- Fixed buffer switch detection to work correctly with non-conformant ASIO drivers

## [0.2.3] - 2025-12-12

### Added
- Added `edition = "2021"` and `rust-version = "1.70"` to Cargo.toml
- Added README.md with usage documentation
- Added CHANGELOG.md following Keep a Changelog format
- Added rustfmt.toml for consistent formatting

### Changed
- Update `bindgen` to 0.72
- Update `cc` to 1.2
- Update `parse_cfg` to 4.1
- Update enumerate example to use `pub type SampleRate = u32` instead of `pub struct SampleRate(pub u32)` for consistency with cpal

### Fixed
- Fix linker flags for MinGW cross-compilation
- Add `packed(4)` to representation of ASIO time structs in bindings
- Fix handling for `kAsioResetRequest` message to prevent driver UI becoming unresponsive
- Fix timeinfo flags type

## [0.2.2] - 2024-03-04

### Added
- Automate ASIO SDK download during build (no longer requires manual download)
- Add support for `CPAL_ASIO_DIR` environment variable to use local SDK

### Changed
- Update `bindgen` to 0.59
- Switch to `once_cell` from `lazy_static`
- Improve build script error messages and SDK detection
- Clean up build script structure
- Re-run build script when `CPAL_ASIO_DIR` changes

### Fixed
- Fix segmentation fault during build on some systems
- Fix various compiler warnings
- Fix typos in code and comments

## [0.2.1] - 2021-11-26

### Changed
- Update `bindgen` to 0.56

### Fixed
- Fix some typos and warnings

## [0.2.0] - 2020-07-22

### Changed
- Update repository URL to https://github.com/RustAudio/cpal/

## [0.1.0] - 2020-07-22

Initial release.

### Added
- FFI bindings to Steinberg ASIO SDK
- Automatic binding generation using bindgen
- Support for MSVC toolchain on Windows
- Basic error types: `AsioError`, `LoadDriverError`

[Unreleased]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.6...HEAD
[0.2.6]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.5...asio-sys-v0.2.6
[0.2.5]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.4...asio-sys-v0.2.5
[0.2.4]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.3...asio-sys-v0.2.4
[0.2.3]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.2...asio-sys-v0.2.3
[0.2.2]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.1...asio-sys-v0.2.2
[0.2.1]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.0...asio-sys-v0.2.1
[0.2.0]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.1.0...asio-sys-v0.2.0
[0.1.0]: https://github.com/RustAudio/cpal/releases/tag/asio-sys-v0.1.0

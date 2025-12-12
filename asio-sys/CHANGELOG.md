# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.3]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.2...asio-sys-v0.2.3
[0.2.2]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.1...asio-sys-v0.2.2
[0.2.1]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.2.0...asio-sys-v0.2.1
[0.2.0]: https://github.com/RustAudio/cpal/compare/asio-sys-v0.1.0...asio-sys-v0.2.0
[0.1.0]: https://github.com/RustAudio/cpal/releases/tag/asio-sys-v0.1.0

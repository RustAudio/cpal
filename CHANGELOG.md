# Unreleased

# Version 0.15.3 (2024-03-04)

- Add `try_with_sample_rate`, a non-panicking variant of `with_sample_rate`.
- struct `platform::Stream` is now #[must_use].
- enum `SupportedBufferSize` and struct `SupportedStreamConfigRange` are now `Copy`.
- `platform::Device` is now `Clone`.
- Remove `parking_lot` dependency in favor of the std library.
- Fix crash on web/wasm when `atomics` flag is enabled.
- Improve Examples: Migrate wasm example to `trunk`, Improve syth-thones example.
- Improve CI: Update actions, Use Android 30 API level in CI, Remove `asmjs-unknown-emscripten` target.
- Update `windows` dependency to v0.54
- Update `jni` dependency to 0.21
- Update `alsa` dependency to 0.9
- Update `oboe` dependency to 0.6
- Update `ndk` dependency to 0.8 and disable `default-features`.
- Update `wasm-bindgen` to 0.2.89

# Version 0.15.2 (2023-03-30)

- webaudio: support multichannel output streams
- Update `windows` dependency
- wasapi: fix some thread panics

# Version 0.15.1 (2023-03-14)

- Add feature `oboe-shared-stdcxx` to enable `shared-stdcxx` on `oboe` for Android support
- Remove `thiserror` dependency
- Swith `mach` dependency to `mach2`

# Version 0.15.0 (2023-01-29)

- Update `windows-rs`, `jack`, `coreaudio-sys`, `oboe`, `alsa` dependencies
- Switch to the `dasp_sample` crate for the sample trait
- Switch to `web-sys` on the emscripten target
- Adopt edition 2021
- Add disconnection detection on Mac OS

# Version 0.14.1 (2022-10-23)

- Support the 0.6.1 release of `alsa-rs`
- Fix `asio` feature broken in 0.14.0
- `NetBSD` support
- CI improvements

# Version 0.14.0 (2022-08-22)

- Switch to `windows-rs` crate
- Turn `ndk-glue` into a dev-dependency and use `ndk-context` instead
- Update dependencies (ndk, ndk-glue, parking_lot, once_cell, jack)

# Version 0.13.5 (2022-01-28)

- Faster sample format conversion
- Update dependencies (ndk, oboe, ndk-glue, jack, alsa, nix)

# Version 0.13.4 (2021-08-08)

- wasapi: Allow both threading models and switch the default to STA
- Update dependencies (core-foundation-sys, jni, rust-jack)
- Alsa: improve stream setup parameters

# Version 0.13.3 (2021-03-29)

- Give each thread a unique name
- Fix distortion regression on some alsa configs

# Version 0.13.2 (2021-03-16)

- Update dependencies (ndk, nix, oboe, jni, etc)

# Version 0.13.1 (2020-11-08)

- Don't panic when device is plugged out on Windows
- Update `parking_lot` dependency

# Version 0.13.0 (2020-10-28)

- Add Android support via `oboe-rs`.
- Add Android APK build an CI job.

# Version 0.12.1 (2020-07-23)

- Bugfix release to get the asio feature working again.

# Version 0.12.0 (2020-07-09)

- Large refactor removing the blocking EventLoop API.
- Rename many `Format` types to `StreamConfig`:
  - `Format` type's `data_type` field renamed to `sample_format`.
  - `Shape` -> `StreamConfig` - The configuration input required to build a stream.
  - `Format` -> `SupportedStreamConfig` - Describes a single supported stream configuration.
  - `SupportedFormat` -> `SupportedStreamConfigRange` - Describes a range of supported configurations.
  - `Device::default_input/output_format` -> `Device::default_input/output_config`.
  - `Device::supported_input/output_formats` -> `Device::supported_input/output_configs`.
  - `Device::SupportedInput/OutputFormats` -> `Device::SupportedInput/OutputConfigs`.
  - `SupportedFormatsError` -> `SupportedStreamConfigsError`
  - `DefaultFormatError` -> `DefaultStreamConfigError`
  - `BuildStreamError::FormatNotSupported` -> `BuildStreamError::StreamConfigNotSupported`
- Address deprecated use of `mem::uninitialized` in WASAPI.
- Removed `UnknownTypeBuffer` in favour of specifying sample type.
- Added `build_input/output_stream_raw` methods allowing for dynamically
  handling sample format type.
- Added support for DragonFly platform.
- Add `InputCallbackInfo` and `OutputCallbackInfo` types and update expected
  user data callback function signature to provide these.

# Version 0.11.0 (2019-12-11)

- Fix some underruns that could occur in ALSA.
- Add name to `HostId`.
- Use `snd_pcm_hw_params_set_buffer_time_near` rather than `set_buffer_time_max`
  in ALSA backend.
- Remove many uses of `std::mem::uninitialized`.
- Fix WASAPI capture logic.
- Panic on stream ID overflow rather than returning an error.
- Use `ringbuffer` crate in feedback example.
- Move errors into a separate module.
- Switch from `failure` to `thiserror` for error handling.
- Add `winbase` winapi feature to solve windows compile error issues.
- Lots of CI improvements.

# Version 0.10.0 (2019-07-05)

- core-foundation-sys and coreaudio-rs version bumps.
- Add an ASIO host, available under Windows.
- Introduce a new Host API, adding support for alternative audio APIs.
- Remove sleep loop on macOS in favour of using a `Condvar`.
- Allow users to handle stream callback errors with a new `StreamEvent` type.
- Overhaul error handling throughout the crate.
- Remove unnecessary Mutex from ALSA and WASAPI backends in favour of channels.
- Remove `panic!` from OutputBuffer Deref impl as it is no longer necessary.

# Version 0.9.0 (2019-06-06)

- Better buffer handling
- Fix logic error in frame/sample size
- Added error handling for unknown ALSA device errors
- Fix resuming a paused stream on Windows (wasapi).
- Implement `default_output_format` for emscripten backend.

# Version 0.8.1 (2018-03-18)

- Fix the handling of non-default sample rates for coreaudio input streams.

# Version 0.8.0 (2018-02-15)

- Add `record_wav.rs` example. Records 3 seconds to
  `$CARGO_MANIFEST_DIR/recorded.wav` using default input device.
- Update `enumerate.rs` example to display default input/output devices and
  formats.
- Add input stream support to coreaudio, alsa and windows backends.
- Introduce `StreamData` type for handling either input or output streams in
  `EventLoop::run` callback.
- Add `Device::supported_{input/output}_formats` methods.
- Add `Device::default_{input/output}_format` methods.
- Add `default_{input/output}_device` functions.
- Replace usage of `Voice` with `Stream` throughout the crate.
- Remove `Endpoint` in favour of `Device` for supporting both input and output
  streams.

# Version 0.7.0 (2018-02-04)

- Rename `ChannelsCount` to `ChannelCount`.
- Rename `SamplesRate` to `SampleRate`.
- Rename the `min_samples_rate` field of `SupportedFormat` to `min_sample_rate`
- Rename the `with_max_samples_rate()` method of`SupportedFormat` to `with_max_sample_rate()`
- Rename the `samples_rate` field of `Format` to `sample_rate`
- Changed the type of the `channels` field of the `SupportedFormat` struct from `Vec<ChannelPosition>` to `ChannelCount` (an alias to `u16`)
- Remove unused ChannelPosition API.
- Implement `Endpoint` and `Format` Enumeration for macOS.
- Implement format handling for macos `build_voice` method.

# Version 0.6.0 (2017-12-11)

- Changed the emscripten backend to consume less CPU.
- Added improvements to the crate documentation.
- Implement `pause` and `play` for ALSA backend.
- Reduced the number of allocations in the CoreAudio backend.
- Fixes for macOS build (#186, #189).

# Version 0.5.1 (2017-10-21)

- Added `Sample::to_i16()`, `Sample::to_u16()` and `Sample::from`.

# Version 0.5.0 (2017-10-21)

- Removed the dependency on the `futures` library.
- Removed the `Voice` and `SamplesStream` types.
- Added `EventLoop::build_voice`, `EventLoop::destroy_voice`, `EventLoop::play`,
  and `EventLoop::pause` that can be used to create, destroy, play and pause voices.
- Added a `VoiceId` struct that is now used to identify a voice owned by an `EventLoop`.
- Changed `EventLoop::run()` to take a callback that is called whenever a voice requires sound data.
- Changed `supported_formats()` to produce a list of `SupportedFormat` instead of `Format`. A
  `SupportedFormat` must then be turned into a `Format` in order to build a voice.

# Unreleased

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
- Implement `Endpoint` and `Format` Enumeration for macos.
- Implement format handling for macos `build_voice` method.

# Version 0.6.0 (2017-12-11)

- Changed the emscripten backend to consume less CPU.
- Added improvements to the crate documentation.
- Implement `pause` and `play` for ALSA backend.
- Reduced the number of allocations in the CoreAudio backend.
- Fixes for macos build (#186, #189).

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

# Unreleased

- Changed the emscripten backend to consume less CPU.
- Added improvements to the crate documentation.
- Implement `pause` and `play` for ALSA backend.

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

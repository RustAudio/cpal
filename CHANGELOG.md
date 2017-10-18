# Unreleased (major)

- Removed the dependency on the `futures` library.
- Removed the `Voice` and `SamplesStream` types.
- Added `EventLoop::build_voice`, `EventLoop::destroy_voice`, `EventLoop::play`,
  and `EventLoop::pause` that can be used to create, destroy, play and pause voices.
- Added a `VoiceId` struct that is now used to identify a voice owned by an `EventLoop`.
- Changed `EventLoop::run()` to take a callback that is called whenever a voice requires sound data.

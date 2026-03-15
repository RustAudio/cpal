# CPAL - Cross-Platform Audio Library

[![Actions Status](https://github.com/RustAudio/cpal/workflows/cpal/badge.svg)](https://github.com/RustAudio/cpal/actions)
[![Crates.io](https://img.shields.io/crates/v/cpal.svg)](https://crates.io/crates/cpal) [![docs.rs](https://docs.rs/cpal/badge.svg)](https://docs.rs/cpal/)

Low-level library for audio input and output in pure Rust.

## Minimum Supported Rust Version (MSRV)

The minimum Rust version required depends on which audio backend and features you're using, as each platform has different dependencies:

- **AAudio (Android):** Rust **1.82**
- **ALSA (Linux/BSD):** Rust **1.82**
- **CoreAudio (macOS/iOS):** Rust **1.80**
- **JACK (Linux/BSD/macOS/Windows):** Rust **1.82**
- **PipeWire (Linux/BSD):** Rust **1.82**
- **PulseAudio (Linux/BSD):** Rust **1.88**
- **WASAPI/ASIO (Windows):** Rust **1.82**
- **WASM (`wasm32-unknown`):** Rust **1.82**
- **WASM (`wasm32-wasip1`):** Rust **1.78**
- **WASM (`audioworklet`):** Rust **nightly** (requires `-Zbuild-std` for atomics support)

## Supported Platforms

This library currently supports the following:

- Enumerate supported audio hosts.
- Enumerate all available audio devices.
- Get the current default input and output devices.
- Enumerate known supported input and output stream formats for a device.
- Get the current default input and output stream formats for a device.
- Build and run input and output PCM streams on a chosen device with a given stream format.

Currently, supported platforms include:

- Android (via AAudio)
- BSD (via ALSA by default, JACK, PipeWire or PulseAudio optionally)
- Emscripten
- iOS (via CoreAudio)
- Linux (via ALSA by default, JACK, PipeWire or PulseAudio optionally)
- macOS (via CoreAudio by default, JACK optionally)
- WebAssembly (via Web Audio API or Audio Worklet)
- Windows (via WASAPI by default, ASIO or JACK optionally)

Note that on Linux, the ALSA development files are required for building (even when using JACK, PipeWire or PulseAudio). These are provided as part of the `libasound2-dev` package on Debian and Ubuntu distributions and `alsa-lib-devel` on Fedora.

## Compiling for WebAssembly

If you are interested in using CPAL with WebAssembly, please see [this guide](https://github.com/RustAudio/cpal/wiki/Setting-up-a-new-CPAL-WASM-project) in our Wiki which walks through setting up a new project from scratch. Some of the examples in this repository also provide working configurations that you can use as reference.

## Optional Features

| Feature | Platform | Description |
|---------|----------|-------------|
| `audio_thread_priority` | Linux, BSD, Windows | Raises the audio callback thread to real-time priority for lower latency and fewer glitches. On Linux, requires `rtkit` or appropriate user permissions (`limits.conf` or capabilities). |
| `asio` | Windows | ASIO backend for low-latency audio, bypassing the Windows audio stack. Requires ASIO drivers and LLVM/Clang. See the [ASIO setup guide](#asio-on-windows). |
| `audioworklet` | WebAssembly (`wasm32-unknown-unknown`) | Audio Worklet backend for lower-latency web audio than the default Web Audio API, running audio on a dedicated thread. Requires atomics support (`RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals"`) and `Cross-Origin` headers for `SharedArrayBuffer`. See the `audioworklet-beep` example. |
| `custom` | All | User-defined host implementations for audio systems not natively supported by CPAL. See `examples/custom.rs`. |
| `jack` | Linux, BSD, macOS, Windows | JACK Audio Connection Kit backend for pro-audio routing and inter-application connectivity. Requires `libjack-jackd2-dev` (Debian/Ubuntu) or `jack-devel` (Fedora). |
| `pipewire` | Linux, BSD | PipeWire media server backend. Requires `libpipewire-0.3-dev` (Debian/Ubuntu) or `pipewire-devel` (Fedora). |
| `pulseaudio` | Linux, BSD | PulseAudio sound server backend. Requires `libpulse-dev` (Debian/Ubuntu) or `pulseaudio-libs-devel` (Fedora). |
| `wasm-bindgen` | WebAssembly (`wasm32-unknown-unknown`) | Web Audio API backend for browser-based audio; required for any WebAssembly audio support. See the `wasm-beep` example. |

See the [beep example](examples/beep.rs) for selecting the host at runtime.

## ASIO on Windows

### Locating the ASIO SDK

The location of ASIO SDK is exposed to CPAL by setting the `CPAL_ASIO_DIR` environment variable.

The build script will try to find the ASIO SDK by following these steps in order:

1. Check if `CPAL_ASIO_DIR` is set and if so use the path to point to the SDK.
2. Check if the ASIO SDK is already installed in the temporary directory, if so use that and set the path of `CPAL_ASIO_DIR` to the output of `std::env::temp_dir().join("asio_sdk")`.
3. If the ASIO SDK is not already installed, download it from <https://www.steinberg.net/asiosdk> and install it in the temporary directory. The path of `CPAL_ASIO_DIR` will be set to the output of `std::env::temp_dir().join("asio_sdk")`.

In an ideal situation you don't need to worry about this step.

### Preparing the Build Environment

1. **Install LLVM/Clang**: `bindgen`, the library used to generate bindings to the C++ SDK, requires clang. Download and install LLVM from <http://releases.llvm.org/download.html> under the "Pre-Built Binaries" section.

2. **Set LIBCLANG_PATH**: Add the LLVM `bin` directory to a `LIBCLANG_PATH` environment variable. If you installed LLVM to the default directory, this should work in the command prompt:
   ```
   setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"
   ```

3. **Install ASIO Drivers** (optional for testing): If you don't have any ASIO devices or drivers available, you can download and install ASIO4ALL from <http://www.asio4all.org/>. Be sure to enable the "offline" feature during installation.

4. **Visual Studio**: The build script assumes Microsoft Visual Studio is installed. It will try to find `vcvarsall.bat` and execute it with the right host and target architecture. If needed, you can manually execute it:
   ```
   "C:\Program Files (x86)\Microsoft Visual Studio\2019\Community\VC\Auxiliary\Build\vcvarsall.bat" amd64
   ```
   For more information see the [vcvarsall.bat documentation](https://docs.microsoft.com/en-us/cpp/build/building-on-the-command-line).

### Using ASIO in Your Application

1. **Enable the feature** in your `Cargo.toml`:
   ```toml
   cpal = { version = "*", features = ["asio"] }
   ```

2. **Select the ASIO host** in your code:
   ```rust
   let host = cpal::host_from_id(cpal::HostId::Asio)
       .expect("failed to initialise ASIO host");
   ```

### Troubleshooting

If you encounter compilation errors from `asio-sys` or `bindgen`:
- Verify `CPAL_ASIO_DIR` is set correctly
- Try running `cargo clean`
- Ensure LLVM/Clang is properly installed and `LIBCLANG_PATH` is set

### Cross-Compilation

When Windows is the host and target OS, the build script supports all cross-compilation targets supported by the MSVC compiler.

It is also possible to compile Windows applications with ASIO support on Linux and macOS using the MinGW-w64 toolchain.

**Requirements:**
- Include the MinGW-w64 include directory in your `CPLUS_INCLUDE_PATH` environment variable
- Include the LLVM include directory in your `CPLUS_INCLUDE_PATH` environment variable

**Example for macOS** (targeting `x86_64-pc-windows-gnu` with `mingw-w64` installed via brew):
```
export CPLUS_INCLUDE_PATH="$CPLUS_INCLUDE_PATH:/opt/homebrew/Cellar/mingw-w64/11.0.1/toolchain-x86_64/x86_64-w64-mingw32/include"
```

## Troubleshooting

### No Default Device Available

If you receive errors about no default input or output device:

- **Linux/PipeWire:** Check that PipeWire is running: `pw-cli info`
- **Linux/PulseAudio:** Check that PulseAudio is running: `pulseaudio --check`
- **macOS:** Check System Preferences > Sound for available devices
- **Mobile (iOS/Android):** Ensure your app has microphone/audio permissions
- **Windows:** Verify your audio device is enabled in Sound Settings

## ALSA, PipeWire, and PulseAudio

When PipeWire or PulseAudio is running, it holds the ALSA `default` device exclusively. A second stream attempting to open it via the ALSA backend will fail with a `DeviceBusy` error. To route audio through the sound server via ALSA, use the bridge devices `pipewire` or `pulse` instead of `default`. Better yet, use the `pipewire` or `pulseaudio` cpal features for native integration.

Reserve `hw:` and `plughw:` device names for targets that have no sound server. On those targets, ensure the user is a member of the `audio` group if the system does not grant audio device access automatically via `logind`.

### Buffer Size Issues

`BufferSize::Default` uses the system-configured device default, which on **ALSA** can range from a PipeWire quantum (typically 1024 frames) to `u32::MAX` on misconfigured or exotic hardware. A very deep buffer causes samples to be consumed far faster than audible playback, making audio appear to fast-forward ahead of actual output.

Configure the system and/or request a fixed size in your application:

| System | File | Setting |
|--------|------|---------|
| ALSA | `~/.asoundrc` or `/etc/asound.conf` | `buffer_size`, `periods` * `period_size` |
| PipeWire | `~/.config/pipewire/pipewire.conf.d/` | `default.clock.quantum` |
| PulseAudio | `~/.config/pulse/daemon.conf` | `default-fragments` * `default-fragment-size-msec` |

```rust
config.buffer_size = cpal::BufferSize::Fixed(1024);
```

Query `device.default_output_config()?.buffer_size()` for valid ranges. Smaller buffers reduce latency but increase CPU load and the risk of glitches.

### Build Errors

If you are unable to build the library:

- Verify you have installed the required development libraries, as documented above
- **ASIO on Windows:** Verify `LIBCLANG_PATH` is set and LLVM is installed

## Examples

CPAL comes with several examples in `examples/`.

Run an example with:
```bash
cargo run --example beep
```

For platform-specific features, enable the relevant features:
```bash
cargo run --example beep --features asio        # Windows ASIO backend
cargo run --example beep --features jack        # JACK backend
cargo run --example beep --features pipewire    # PipeWire backend
cargo run --example beep --features pulseaudio  # PulseAudio backend
```

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## Resources

- **Documentation:** [docs.rs/cpal](https://docs.rs/cpal)
- **Examples:** [examples/](examples/) directory in this repository
- **Discord:** Join the [#cpal channel](https://discord.gg/vPmmSgJSPV) for questions and discussion
- **GitHub:** [Report issues](https://github.com/RustAudio/cpal/issues) and [view source code](https://github.com/RustAudio/cpal)
- **RustAudio:** Part of the [RustAudio organization](https://github.com/RustAudio)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

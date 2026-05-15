# CPAL - Cross-Platform Audio Library

[![Actions Status](https://github.com/RustAudio/cpal/workflows/cpal/badge.svg)](https://github.com/RustAudio/cpal/actions)
[![Crates.io](https://img.shields.io/crates/v/cpal.svg)](https://crates.io/crates/cpal) [![docs.rs](https://docs.rs/cpal/badge.svg)](https://docs.rs/cpal/)

Low-level library for audio input and output in pure Rust.

## Supported Functionality

- Enumerate audio hosts, devices, and their supported stream configurations.
- Look up devices by stable ID or by default input/output role.
- Inspect device metadata: name, manufacturer, type, and bus type.       
- Build input and output streams with compile-time or runtime sample formats.
- Play, pause, and query the buffer size and clock of a stream.

## Supported Platforms

- Android (via AAudio)
- BSD (via ALSA by default, JACK, PipeWire or PulseAudio optionally)
- iOS (via CoreAudio)
- Linux (via ALSA by default, JACK, PipeWire or PulseAudio optionally)
- macOS (via CoreAudio by default, JACK optionally)
- tvOS (via CoreAudio)
- WebAssembly (via Web Audio API or Audio Worklet)
- Windows (via WASAPI by default, ASIO or JACK optionally)

## Linux Build Dependencies

On Linux, building cpal requires the ALSA and D-Bus development files: `libasound2-dev` and 
`libdbus-1-dev` on Debian and Ubuntu, `alsa-lib-devel` and `dbus-devel` on Fedora.

ALSA is needed even when using JACK, PipeWire, or PulseAudio.

D-Bus is pulled in by the default `realtime-dbus` feature for `rtkit`-based RT scheduling, typical
for desktop systems. For systems without D-Bus, disable default features and enable the plain
`realtime` feature instead. See [Real-Time Priority Promotion](#real-time-priority-promotion).
Disable both features to disable RT scheduling entirely.

## Minimum Supported Rust Version (MSRV)

The minimum Rust version required depends on which audio backend and features you're using, as each platform has different dependencies:

- **AAudio (Android):** Rust **1.85**
- **ALSA (Linux/BSD):** Rust **1.82**
- **CoreAudio (macOS/iOS/tvOS):** Rust **1.85**
- **JACK (Linux/BSD/macOS/Windows):** Rust **1.82**
- **PipeWire (Linux/BSD):** Rust **1.85**
- **PulseAudio (Linux/BSD):** Rust **1.88**
- **WASAPI/ASIO (Windows):** Rust **1.82**
- **WASM (`wasm32-unknown`):** Rust **1.85**
- **WASM (`wasm32-wasip1`):** Rust **1.78**
- **WASM (`audioworklet`):** Rust **nightly** (requires `-Zbuild-std` for atomics support)

## Compiling for WebAssembly

If you are interested in using CPAL with WebAssembly, please see [this guide](https://github.com/RustAudio/cpal/wiki/Setting-up-a-new-CPAL-WASM-project) in our Wiki which walks through setting up a new project from scratch. Some of the examples in this repository also provide working configurations that you can use as reference.

## Optional Features

| Feature | Platform | Description |
|---------|----------|-------------|
| `asio` | Windows | ASIO backend for low-latency audio, bypassing the Windows audio stack. Requires ASIO drivers and LLVM/Clang. See the [ASIO setup guide](#asio). |
| `audioworklet` | WebAssembly (`wasm32-unknown-unknown`) | Audio Worklet backend for lower-latency web audio than the default Web Audio API, running audio on a dedicated thread. Requires atomics support (`RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals"`) and `Cross-Origin` headers for `SharedArrayBuffer`. See the `audioworklet-beep` example. |
| `custom` | All | User-defined backend implementations for audio systems not natively supported by CPAL. See `examples/custom.rs`. |
| `jack` | Linux, BSD, macOS, Windows | JACK Audio Connection Kit backend for pro-audio routing and inter-application connectivity. Requires `libjack-jackd2-dev` (Debian/Ubuntu) or `jack-devel` (Fedora). |
| `pipewire` | Linux, BSD | PipeWire media server backend. Requires `libpipewire-0.3-dev` (Debian/Ubuntu) or `pipewire-devel` (Fedora). |
| `pulseaudio` | Linux, BSD | PulseAudio sound server backend. Requires `libpulse-dev` (Debian/Ubuntu) or `pulseaudio-libs-devel` (Fedora). |
| `realtime` | Linux, BSD, Windows, Android | Raises the audio callback thread to real-time or high-priority scheduling for lower latency. On Linux/BSD, requires `rtprio` granted in `limits.conf` (e.g. `@audio - rtprio 95`) unless `realtime-dbus` is also enabled. |
| `realtime-dbus` | Linux, BSD, Windows, Android | Uses `rtkit` via D-Bus for RT scheduling on Linux/BSD desktop systems, removing the need for manual `limits.conf` setup. Implies `realtime` on all platforms. Enabled by default. |
| `wasm-bindgen` | WebAssembly (`wasm32-unknown-unknown`) | Web Audio API backend for browser-based audio; required for any WebAssembly audio support. See the `wasm-beep` example. |

See the [beep example](examples/beep.rs) for selecting the backend at runtime.

## ASIO

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

2. **Select the ASIO backend** in your code:
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
```sh
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

When PipeWire or PulseAudio is running, it holds the ALSA `default` device exclusively. A second
stream attempting to open it via the ALSA host will fail with a `DeviceBusy` error. To route
audio through the sound server via ALSA, use the bridge devices `pipewire` or `pulse` instead of
`default`. Better yet, use the `pipewire` or `pulseaudio` cpal features for native integration.

On targets without a sound server, address devices directly as `hw:` or `plughw:`.

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

### ALSA Real-Time Priority Promotion

The ALSA backend refuses to promote the audio thread to RT priority for plugins such as `pcm.pulse`
and `pcm.pipewire`, notifying `RealtimeDenied` after stream creation on the error callback.
Consider using the `pulseaudio` or `pipewire` cpal features to open the device through the native
backend instead. While RT priority is desirable for low latency, the stream will continue to play 
at the default scheduling priority.

Kernel-backed PCMs (`hw`, `plughw`) and pure-computation plugins are unaffected.

`RealtimeDenied` is also received when the process lacks the resource limits to acquire
`SCHED_FIFO`. With the default `realtime-dbus` feature, `rtkit` arranges this over D-Bus on 
typical desktop systems. With the plain `realtime` feature, you must ensure that `rtprio` is 
granted yourself. Add to `/etc/security/limits.d/audio.conf` and ensure the user is member of the 
`audio` group:

```
@audio - rtprio 95
```

then add the user to the `audio` group (`usermod -aG audio "$USER"`) and re-login. The same group
may anyway be needed to grant access to ALSA device files via `udev` on systems that do not
arrange this automatically via `logind`.

### Build Errors

If you are unable to build the library:

- Verify you have installed the required development libraries, as documented above
- **ASIO on Windows:** Verify `LIBCLANG_PATH` is set and LLVM is installed

## Examples

CPAL comes with several examples in `examples/`.

Run an example with:
```sh
cargo run --example beep
```

For platform-specific features, enable the relevant features:
```sh
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

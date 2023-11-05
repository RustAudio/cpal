# CPAL - Cross-Platform Audio Library

[![Actions Status](https://github.com/RustAudio/cpal/workflows/cpal/badge.svg)](https://github.com/RustAudio/cpal/actions)
[![Crates.io](https://img.shields.io/crates/v/cpal.svg)](https://crates.io/crates/cpal) [![docs.rs](https://docs.rs/cpal/badge.svg)](https://docs.rs/cpal/)

Low-level library for audio input and output in pure Rust.

This library currently supports the following:

- Enumerate supported audio hosts.
- Enumerate all available audio devices.
- Get the current default input and output devices.
- Enumerate known supported input and output stream formats for a device.
- Get the current default input and output stream formats for a device.
- Build and run input and output PCM streams on a chosen device with a given stream format.

Currently, supported hosts include:

- Linux (via ALSA or JACK)
- Windows (via WASAPI by default, see ASIO instructions below)
- macOS (via CoreAudio)
- iOS (via CoreAudio)
- Android (via Oboe)
- Emscripten

Note that on Linux, the ALSA development files are required. These are provided
as part of the `libasound2-dev` package on Debian and Ubuntu distributions and
`alsa-lib-devel` on Fedora.

## Compiling for Web Assembly

If you are interested in using CPAL with WASM, please see [this guide](https://github.com/RustAudio/cpal/wiki/Setting-up-a-new-CPAL-WASM-project) in our Wiki which walks through setting up a new project from scratch.

## Feature flags for audio backends

Some audio backends are optional and will only be compiled with a [feature flag](https://doc.rust-lang.org/cargo/reference/features.html).

- JACK (on Linux): `jack`
- ASIO (on Windows): `asio`

Oboe can either use a shared or static runtime. The static runtime is used by default, but activating the
`oboe-shared-stdcxx` feature makes it use the shared runtime, which requires `libc++_shared.so` from the Android NDK to
be present during execution.

## ASIO on Windows

[ASIO](https://en.wikipedia.org/wiki/Audio_Stream_Input/Output) is an audio
driver protocol by Steinberg. While it is available on multiple operating
systems, it is most commonly used on Windows to work around limitations of
WASAPI including access to large numbers of channels and lower-latency audio
processing.

CPAL allows for using the ASIO SDK as the audio host on Windows instead of
WASAPI.

### Locating the ASIO SDK

The location of ASIO SDK is exposed to CPAL by setting the `CPAL_ASIO_DIR` environment variable.

The build script will try to find the ASIO SDK by following these steps in order:

1. Check if `CPAL_ASIO_DIR` is set and if so use the path to point to the SDK.
2. Check if the ASIO SDK is already installed in the temporary directory, if so use that and set the path of `CPAL_ASIO_DIR` to the output of `std::env::temp_dir().join("asio_sdk")`.
3. If the ASIO SDK is not already installed, download it from <https://www.steinberg.net/asiosdk> and install it in the temporary directory. The path of `CPAL_ASIO_DIR` will be set to the output of `std::env::temp_dir().join("asio_sdk")`.

In an ideal situation you don't need to worry about this step.

### Preparing the build environment

1. `bindgen`, the library used to generate bindings to the C++ SDK, requires
   clang. **Download and install LLVM** from
   [here](http://releases.llvm.org/download.html) under the "Pre-Built Binaries"
   section. The version as of writing this is 17.0.1.
2. Add the LLVM `bin` directory to a `LIBCLANG_PATH` environment variable. If
   you installed LLVM to the default directory, this should work in the command
   prompt:
   ```
   setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"
   ```
3. If you don't have any ASIO devices or drivers available, you can [**download
   and install ASIO4ALL**](http://www.asio4all.org/). Be sure to enable the
   "offline" feature during installation despite what the installer says about
   it being useless.
4. Our build script assumes that Microsoft Visual Studio is installed if the host OS for compilation is Windows. The script will try to find `vcvarsall.bat`
   and execute it with the right host and target machine architecture regardless of the Microsoft Visual Studio version.
   If there are any errors encountered in this process which is unlikely,
   you may find the `vcvarsall.bat` manually and execute it with your machine architecture as an argument.
   The script will detect this and skip the step.

   A manually executed command example for 64 bit machines:

   ```
   "C:\Program Files (x86)\Microsoft Visual Studio\2019\Community\VC\Auxiliary\Build\vcvarsall.bat" amd64
   ```

   For more information please refer to the documentation of [`vcvarsall.bat``](https://docs.microsoft.com/en-us/cpp/build/building-on-the-command-line?view=msvc-160#vcvarsall-syntax).

5. Select the ASIO host at the start of our program with the following code:

   ```rust
   let host;
   #[cfg(target_os = "windows")]
   {
      host = cpal::host_from_id(cpal::HostId::Asio).expect("failed to initialise ASIO host");
   }
   ```

   If you run into compilations errors produced by `asio-sys` or `bindgen`, make
   sure that `CPAL_ASIO_DIR` is set correctly and try `cargo clean`.

6. Make sure to enable the `asio` feature when building CPAL:

   ```
   cargo build --features "asio"
   ```

   or if you are using CPAL as a dependency in a downstream project, enable the
   feature like this:

   ```toml
   cpal = { version = "*", features = ["asio"] }
   ```

_Updated as of ASIO version 2.3.3._

### Cross compilation

When Windows is the host and the target OS, the build script of `asio-sys` supports all cross compilation targets
which are supported by the MSVC compiler. An exhaustive list of combinations could be found [here](https://docs.microsoft.com/en-us/cpp/build/building-on-the-command-line?view=msvc-160#vcvarsall-syntax) with the addition of undocumented `arm64`, `arm64_x86`, `arm64_amd64` and `arm64_arm` targets. (5.11.2023)

It is also possible to compile Windows applications with ASIO support on Linux and macOS.

For both platforms the common way to do this is to use the [MinGW-w64](https://www.mingw-w64.org/) toolchain.

Make sure that you have included the `MinGW-w64` include directory in your `CPLUS_INCLUDE_PATH` environment variable.
Make sure that LLVM is installed and include directory is also included in your `CPLUS_INCLUDE_PATH` environment variable.

Example for macOS for the target of `x86_64-pc-windows-gnu` where `mingw-w64` is installed via brew:

```
export CPLUS_INCLUDE_PATH="$CPLUS_INCLUDE_PATH:/opt/homebrew/Cellar/mingw-w64/11.0.1/toolchain-x86_64/x86_64-w64-mingw32/include"
```

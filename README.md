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

Currently supported hosts include:

- Linux (via ALSA)
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

## ASIO on Windows

[ASIO](https://en.wikipedia.org/wiki/Audio_Stream_Input/Output) is an audio
driver protocol by Steinberg. While it is available on multiple operating
systems, it is most commonly used on Windows to work around limitations of
WASAPI including access to large numbers of channels and lower-latency audio
processing.

CPAL allows for using the ASIO SDK as the audio host on Windows instead of
WASAPI. To do so, follow these steps:

1. **Download the ASIO SDK** `.zip` from [this
   link](https://www.steinberg.net/en/company/developers.html). The version as
   of writing this is 2.3.1.
2. Extract the files and place the directory somewhere you are happy for it to stay
   (e.g. `~/.asio`).
3. Assign the full path of the directory (that contains the `readme`, `changes`,
   `ASIO SDK 2.3` pdf, etc) to the `CPAL_ASIO_DIR` environment variable. This is
   necessary for the `asio-sys` build script to build and bind to the SDK.
4. `bindgen`, the library used to generate bindings to the C++ SDK, requires
   clang. **Download and install LLVM** from
   [here](http://releases.llvm.org/download.html) under the "Pre-Built Binaries"
   section. The version as of writing this is 7.0.0.
5. Add the LLVM `bin` directory to a `LIBCLANG_PATH` environment variable. If
   you installed LLVM to the default directory, this should work in the command
   prompt:
   ```
   setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"
   ```
6. If you don't have any ASIO devices or drivers available, you can [**download
   and install ASIO4ALL**](http://www.asio4all.org/). Be sure to enable the
   "offline" feature during installation despite what the installer says about
   it being useless.
7. **Loading VCVARS**. `rust-bindgen` uses the C++ tool-chain when generating
   bindings to the ASIO SDK. As a result, it is necessary to load some
   environment variables in the command prompt that we use to build our project.
   On 64-bit machines run:
   ```
   "C:\Program Files (x86)\Microsoft Visual Studio 14.0\VC\vcvarsall.bat" amd64
   ```
   On 32-bit machines run:
   ```
   "C:\Program Files (x86)\Microsoft Visual Studio 14.0\VC\vcvarsall.bat" x86
   ```
   Note that, depending on your version of Visual Studio, this script might be
   in a slightly different location.
8. Select the ASIO host at the start of our program with the following code:

   ```rust
   let host;
   #[cfg(target_os = "windows")]
   {
       host = cpal::host_from_id(cpal::HostId::Asio).expect("failed to initialise ASIO host");
   }
   ```

   If you run into compilations errors produced by `asio-sys` or `bindgen`, make
   sure that `CPAL_ASIO_DIR` is set correctly and try `cargo clean`.
9. Make sure to enable the `asio` feature when building CPAL:

   ```
   cargo build --features "asio"
   ```

   or if you are using CPAL as a dependency in a downstream project, enable the
   feature like this:

   ```toml
   cpal = { version = "*", features = ["asio"] }
   ```

In the future we would like to work on automating this process to make it
easier, but we are not familiar enough with the ASIO license to do so yet.

*Updated as of ASIO version 2.3.3.*

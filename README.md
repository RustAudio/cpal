# CPAL - Cross-Platform Audio Library

[![Build Status](https://travis-ci.org/tomaka/cpal.svg?branch=master)](https://travis-ci.org/tomaka/cpal) [![Crates.io](https://img.shields.io/crates/v/cpal.svg)](https://crates.io/crates/cpal) [![docs.rs](https://docs.rs/cpal/badge.svg)](https://docs.rs/cpal/)

Low-level library for audio input and output in pure Rust.

This library currently supports the following:

- Enumerate all available audio devices.
- Get the current default input and output devices.
- Enumerate known supported input and output stream formats for a device.
- Get the current default input and output stream formats for a device.
- Build and run input and output PCM streams on a chosen device with a given stream format.

Currently supported backends include:

- Linux (via ALSA)
- Windows (via WASAPI by default, see ASIO instructions below)
- macOS (via CoreAudio)
- iOS (via CoreAudio)
- Emscripten

Note that on Linux, the ALSA development files are required. These are provided
as part of the `libasound2-dev` package on Debian and Ubuntu distributions and
`alsa-lib-devel` on Fedora.

## ASIO on Windows

[ASIO](https://en.wikipedia.org/wiki/Audio_Stream_Input/Output) is an audio
driver protocol by Steinberg. While it is available on multiple operating
systems, it is most commonly used on Windows to work around limitations of
WASAPI including access to large numbers of channels and lower-latency audio
processing.

CPAL allows for using the ASIO SDK as the audio backend on Windows instead of
WASAPI. To do so, follow these steps:

1. **Download the ASIO SDK** `.zip` from [this
   link](https://www.steinberg.net/en/company/developers.html). The version as
   of writing this is 2.3.1.
2. Extract the files and place the `ASIOSDK2.3.1` directory somewhere you are
   happy for it to stay (e.g. `~/.asio`).
3. Assign the full path of the `ASIOSDK2.3.1` directory to the `CPAL_ASIO_DIR`
   environment variable. [How to set persisting Environment Variables on
   Windows](https://gist.github.com/mitchmindtree/92c8e37fa80c8dddee5b94fc88d1288b#file-windows_environment_variables-md).
4. **Download and install LLVM** from
   [here](http://releases.llvm.org/download.html) under the "Pre-Built Binaries"
   section. The version as of writing this is 7.0.0.
5. Add the LLVM `bin` directory to a `LIBCLANG_PATH` environment variable. If
   you installed LLVM to the default directory, this should work in the command
   prompt:
   ```
   setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"
   ```
6. If you don't have any ASIO devices or drivers availabe, you can [**download
   and install ASIO4ALL**](http://www.asio4all.org/). Be sure to enable the
   "offline" feature during installation despite what the installer says about
   it being useless.
7. **Use the correct command prompt** to build cpal and run examples. In my
   case, I had to run a specific command prompt, otherwise rust-bindgen would
   fail to find some of the necessary build tools. To do this, I went to the
   `Start Menu > Visual C++ Build Tools > Visual C++ 2015 x64 Native Build Tools
   Command Prompt` and ran this prompt. The exact prompt you need might differ
   based on your machine's architecture and how you installed your Visual C++
   tools. There must be an easier solution to this (especially as not everyone
   wants to build projects from the command line).
8. Select ASIO as the backend at the start of our program with the following:
   
   ```rust
   #[cfg(target_os = "windows")]
   {
       cpal::os::windows::use_asio_backend().expect("Failed to select ASIO backend");
   }
   ```

   If you run into this error:

   ```
   cpal::os::windows::use_asio_backend().expect("Failed to use asio");
                      ^^^^^^^^^^^^^^^^ did you mean `use_wasapi_backend`?
   ```

   Make sure that `CPAL_ASIO_DIR` is set correctly and try `cargo clean`.

In the future we would like to work on automating this process to make it
easier, but we are not familiar enough with the ASIO license to do so yet.

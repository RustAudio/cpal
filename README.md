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
- Windows
- macOS (via CoreAudio)
- iOS (via CoreAudio)
- Emscripten

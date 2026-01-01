# asio-sys

[![Crates.io](https://img.shields.io/crates/v/asio-sys.svg)](https://crates.io/crates/asio-sys)
[![Documentation](https://docs.rs/asio-sys/badge.svg)](https://docs.rs/asio-sys)
[![License](https://img.shields.io/crates/l/asio-sys.svg)](https://github.com/RustAudio/cpal/blob/master/LICENSE)

Low-level Rust bindings for the [Steinberg ASIO SDK](https://www.steinberg.net/developers/).

ASIO (Audio Stream Input/Output) is a low-latency audio API for Windows that provides direct hardware access, bypassing the Windows audio stack for minimal latency.

## Overview

`asio-sys` provides raw FFI bindings to the ASIO SDK, automatically generated using [bindgen](https://rust-lang.github.io/rust-bindgen/). This crate is used by [cpal](https://crates.io/crates/cpal)'s ASIO backend to provide low-latency audio on Windows.

**Note:** Most users should use [cpal](https://crates.io/crates/cpal)'s safe, cross-platform API rather than using `asio-sys` directly.

## Features

- Automatic binding generation from ASIO SDK headers
- Low-level access to ASIO driver functionality
- Support for both MSVC and MinGW toolchains
- Automated ASIO SDK download and setup during build

## Requirements

### Windows

- **LLVM/Clang**: Required for bindgen to generate bindings
  - Install via [LLVM downloads](https://releases.llvm.org/) or `choco install llvm`
- **ASIO SDK**: Automatically downloaded during build from Steinberg
  - Or set `CPAL_ASIO_DIR` environment variable to point to a local SDK

### Build Dependencies

- `bindgen` - Generates Rust bindings from C/C++ headers
- `cc` - Compiles the ASIO SDK C++ files
- `walkdir` - Finds SDK files

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
asio-sys = "0.2"
```

### Example

```rust
use asio_sys as sys;

fn main() {
    // Load ASIO driver
    let driver_name = "ASIO4ALL v2"; // Your ASIO driver name

    unsafe {
        // Initialize ASIO
        let drivers = sys::get_driver_names();
        println!("Available drivers: {drivers:?}");

        // Load a driver
        match sys::load_asio_driver(driver_name) {
            Ok(driver) => println!("Loaded driver: {driver_name}"),
            Err(e) => eprintln!("Failed to load driver: {e:?}"),
        }
    }
}
```

## Environment Variables

- `CPAL_ASIO_DIR`: Path to ASIO SDK directory (optional)
  - If not set, the SDK is automatically downloaded during build
  - Example: `set CPAL_ASIO_DIR=C:\path\to\asiosdk`

## Platform Support

- **Windows** (MSVC and MinGW)
  - x86_64 (64-bit)
  - i686 (32-bit)

ASIO is Windows-only. This crate will not build on other platforms.

## Safety

This crate provides raw FFI bindings to C++ code. Almost all functions are `unsafe` and require careful handling:

- Memory management is manual
- Callbacks must be properly synchronized
- Driver state must be carefully managed
- See [ASIO SDK documentation](https://www.steinberg.net/developers/) for details

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../LICENSE) for details.

The ASIO SDK is owned by Steinberg Media Technologies GmbH. Users must comply with Steinberg's licensing terms.

## Contributing

Contributions are welcome! Please submit issues and pull requests to the [cpal repository](https://github.com/RustAudio/cpal).

## Resources

- [ASIO SDK Documentation](https://www.steinberg.net/developers/)
- [cpal Documentation](https://docs.rs/cpal)
- [RustAudio Community](https://github.com/RustAudio)
- [Discord](https://discord.gg/vPmmSgJSPV): #cpal Channel

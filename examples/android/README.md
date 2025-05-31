## How to install

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
```

## How to build apk

```sh
# Builds the project in release mode and places it into a `apk` file.
cargo apk build --release
```

more information at: https://github.com/rust-mobile/cargo-apk
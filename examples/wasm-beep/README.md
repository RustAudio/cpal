## How to install

[trunk](https://trunkrs.dev/) is used to build and serve the example.
```sh
cargo install --locked trunk
# -- or --
cargo binstall trunk
```

## How to run in debug mode

```sh
# Builds the project and opens it in a new browser tab. Auto-reloads when the project changes.
trunk serve --open
```

## How to build in release mode

```sh
# Builds the project in release mode and places it into the `dist` folder.
trunk build --release
```

## What does each file do?

* `Cargo.toml` contains the standard Rust metadata. You put your Rust dependencies in here. You must change this file with your details (name, description, version, authors, categories)

* The `src` folder contains your Rust code.

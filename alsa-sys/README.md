# alsa-sys

Low-level bindings to the ALSA lib generated via bindgen (v0.52).

Command used for bindgings generation:

```bash
cd cpal/alsa-sys
bindgen --default-enum-style rust ${ALSA_LIB_DEV}/include/alsa/asoundlib.h -o src/lib.rs
```

Where `ALSA_LIB_DEV` is an environment variable pointing to the ALSA headers on
my NixOS system.

The following lines were added to the top of the file to avoid unnecessary
warnings:

```rust
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
```

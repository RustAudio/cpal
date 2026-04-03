# Upgrading from v0.17 to v0.18

This guide covers breaking changes requiring code updates. See [CHANGELOG.md](CHANGELOG.md) for the complete list of changes and improvements.

## Breaking Changes Checklist

- [ ] Add wildcard arms to exhaustive `match` expressions on cpal error enums
- [ ] Optionally handle the new `DeviceBusy` variant for retryable device errors
- [ ] Change `build_*_stream` call sites to pass `StreamConfig` by value (drop the `&`)
- [ ] For custom hosts, change `DeviceTrait` implementations to accept `StreamConfig` by value.
- [ ] Remove `instant.duration_since(e)` unwraps; it now returns `Duration` (saturating).
- [ ] Change `instant.add(d)` to `instant.checked_add(d)` (or use `instant + d`).
- [ ] Change `instant.sub(d)` to `instant.checked_sub(d)` (or use `instant - d`).
- [ ] Update `StreamInstant::new(secs, nanos)` call sites: `secs` is now `u64`.
- [ ] Update `StreamInstant::from_nanos(nanos)` call sites: `nanos` is now `u64`.
- [ ] Update `duration_since` call sites to pass by value (drop the `&`).

## 1. Error enums are now `#[non_exhaustive]`

**What changed:** Public error enums in `cpal` are now marked `#[non_exhaustive]`.

```rust
// Before (v0.17)
match device.default_output_config() {
    Ok(config) => config,
    Err(DefaultStreamConfigError::DeviceNotAvailable) => panic!("device gone"),
    Err(DefaultStreamConfigError::StreamTypeNotSupported) => panic!("unsupported"),
    Err(DefaultStreamConfigError::BackendSpecific { err }) => panic!("{err}"),
}

// After (v0.18)
loop {
    match device.default_output_config() {
        Ok(config) => break config,
        Err(DefaultStreamConfigError::DeviceBusy) => {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err(DefaultStreamConfigError::DeviceNotAvailable) => panic!("device gone"),
        Err(DefaultStreamConfigError::StreamTypeNotSupported) => panic!("unsupported"),
        Err(DefaultStreamConfigError::BackendSpecific { err }) => panic!("{err}"),
        Err(_) => panic!("unknown error"),
    }
}
```

**Why:** This lets cpal add new variants in future minor releases without a SemVer-breaking change.

## 2. New `DeviceBusy` variant

**What changed:** On ALSA, `EBUSY`/`EAGAIN` errors from device open calls now produce `DeviceBusy` instead of `DeviceNotAvailable`. This may be added to other hosts in the future.

**Why:** Unlike `DeviceNotAvailable` (device is gone), `DeviceBusy` signals a transient condition. Retrying after a short delay may succeed, as shown in the example above.

## 3. `StreamConfig` is now passed by value

**What changed:** `StreamConfig` now implements `Copy`, and all `DeviceTrait` stream-building methods accept it by value.

```rust
// Before (v0.17)
let stream = device.build_output_stream(&config, data_fn, err_fn, None)?;

// After (v0.18)
let stream = device.build_output_stream(config, data_fn, err_fn, None)?;
```

**Impact:** Remove the `&` at every `build_*_stream` call site. Because `StreamConfig` is `Copy`, you can reuse the same binding across multiple calls without cloning.

If you implement `DeviceTrait` on your own type (via the `custom` feature), update your `build_input_stream_raw` and `build_output_stream_raw` signatures from `config: &StreamConfig` to `config: StreamConfig`. Any `config.clone()` calls before `move` closures can also be removed.

## 4. `StreamInstant` API overhaul

The `StreamInstant` API has been aligned with `std::time::Instant` and `std::time::Duration`.

### `duration_since` now returns `Duration` (saturating)

**What changed:** `duration_since` now returns `Duration` directly, saturating to `Duration::ZERO`
when the argument is later than `self`, instead of returning `Option<Duration>`.

```rust
// Before (v0.17): returned Option<Duration>, argument by reference
if let Some(d) = callback.duration_since(&start) {
    println!("elapsed: {d:?}");
}

// After (v0.18): returns Duration (saturating), argument by value
let d = callback.duration_since(start);
println!("elapsed: {d:?}");

// For the previous Option-returning behaviour, use checked_duration_since:
if let Some(d) = callback.checked_duration_since(start) {
    println!("elapsed: {d:?}");
}
```

**Why:** Mirrors the saturating behavior of `std::time::Instant::saturating_duration_since` in the Rust standard library.

### `add` / `sub` renamed to `checked_add` / `checked_sub`; operator impls added

**What changed:** The `add` and `sub` methods (which returned `Option`) are replaced by
`checked_add` / `checked_sub` with the same semantics. `+`, `-`, `+=`, and `-=` operator impls
are also added.

```rust
// Before (v0.17)
let future = instant.add(Duration::from_millis(10)).expect("overflow");
let past   = instant.sub(Duration::from_millis(10)).expect("underflow");

// After (v0.18): explicit checked form (same semantics):
let future = instant.checked_add(Duration::from_millis(10)).expect("overflow");
let past   = instant.checked_sub(Duration::from_millis(10)).expect("underflow");

// Or use the operator (panics on overflow, like std::time::Instant):
let future = instant + Duration::from_millis(10);
let past   = instant - Duration::from_millis(10);

// Subtract two instants to get a Duration (saturates to zero):
let elapsed: Duration = later - earlier;
```

**Why:** Aligns the API with `std::time::Instant`, making `StreamInstant` more idiomatic.

### `new` and `from_nanos` take unsigned integers

**What changed:** The `secs` parameter of `StreamInstant::new` and the `nanos` parameter of
`StreamInstant::from_nanos` are now `u64` instead of `i64`.

```rust
// Before (v0.17): negative seconds were accepted
StreamInstant::new(-1_i64, 0);

// After (v0.18): all stream clocks are non-negative
StreamInstant::new(0_u64, 0);
```

**Why:** All audio host clocks are positive and monotonic; they are never negative.

---

# Upgrading from v0.16 to v0.17

## Breaking Changes Checklist

- [ ] Replace `SampleRate(n)` with plain `n` values
- [ ] Update `windows` crate to >= 0.59, <= 0.62 (Windows only)
- [ ] Update `alsa` crate to 0.11 (Linux only)
- [ ] Remove `wee_alloc` feature from Wasm builds (if used)
- [ ] Wrap CoreAudio streams in `Arc` if you were cloning them (macOS only)
- [ ] Handle `BuildStreamError::StreamConfigNotSupported` for `BufferSize::Fixed` (JACK, strict validation)
- [ ] Update device name matching if using ALSA (Linux only)

**Recommended migrations:**
- [ ] Replace deprecated `device.name()` calls with `device.description()` or `device.id()`

---

## 1. SampleRate is now a u32 type alias

**What changed:** `SampleRate` changed from a struct to a `u32` type alias.

```rust
// Before (v0.16)
use cpal::SampleRate;
let config = StreamConfig {
    channels: 2,
    sample_rate: SampleRate(44100),
    buffer_size: BufferSize::Default,
};

// After (v0.17)
let config = StreamConfig {
    channels: 2,
    sample_rate: 44100,
    buffer_size: BufferSize::Default,
};
```

**Impact:** Remove `SampleRate()` constructor calls. The type is now just `u32`, so use integer literals or variables directly.

## 2. Device::name() deprecated (soft deprecation)

**What changed:** `Device::name()` is deprecated in favor of `id()` and `description()`.

```rust
// Old (still works but shows deprecation warning)
let name = device.name()?;

// New: For user-facing display
let desc = device.description()?;
println!("Device: {}", desc);  // or desc.name() for just the name

// New: For stable identification and persistence
let id = device.id()?;
let id_string = id.to_string();  // Save this
// Later...
let device = host.device_by_id(&id_string.parse()?)?;
```

**Impact:** Deprecation warnings only. The old API still works in v0.17. Update when convenient to prepare for future versions.

**Why:** Separates stable device identification (`id()`) from human-readable names (`description()`).

## 3. CoreAudio Stream no longer Clone (macOS)

**What changed:** On macOS, `Stream` no longer implements `Clone`. Use `Arc` instead.

```rust
// Before (v0.16) - macOS only
let stream = device.build_output_stream(&config, data_fn, err_fn, None)?;
let stream_clone = stream.clone();

// After (v0.17) - all platforms
let stream = Arc::new(device.build_output_stream(&config, data_fn, err_fn, None)?);
let stream_clone = Arc::clone(&stream);
```

**Why:** Removed as part of making `Stream` implement `Send` on macOS.

## 4. BufferSize behavior changes

### BufferSize::Default now uses host defaults

**What changed:** `BufferSize::Default` now defers to the audio host/device defaults instead of applying cpal's opinionated defaults.

**Impact:** Buffer sizes may differ from v0.16, affecting latency characteristics:
- **Latency will vary** based on host/device defaults (which may be lower, higher, or similar)
- **May underrun or have different latency** depending on what the host chooses
- **Better integration** with system audio configuration: cpal now respects configured settings instead of imposing its own buffers. For example, on ALSA, PipeWire quantum settings (via the pipewire-alsa device) are now honored instead of being overridden.

**Migration:** If you experience underruns, fast-forwarding behavior or need specific latency, use `BufferSize::Fixed(size)` instead of relying on possibly misconfigured system defaults.

**Platform-specific notes:**
- **ALSA:** Previously used cpal's hardcoded 25ms periods / 100ms buffer, now uses device defaults
- **All platforms:** Default buffer sizes now match what the host audio system expects

### BufferSize::Fixed validation changes

**What changed:** Several backends now have different validation behavior for `BufferSize::Fixed`:

- **ALSA:** Now uses `set_buffer_size_near()` for improved hardware compatibility with devices requiring byte-alignment, power-of-two sizes, or other alignment constraints (was: exact size via `set_buffer_size()`, which would reject unsupported sizes)
- **JACK:** Must exactly match server buffer size (was: silently ignored)
- **Emscripten/WebAudio:** Validates min/max range
- **ASIO:** Stricter lower bound validation

```rust
// Handle validation errors
let mut config = StreamConfig {
    channels: 2,
    sample_rate: 44100,
    buffer_size: BufferSize::Fixed(512),
};

match device.build_output_stream(&config, data_fn, err_fn, None) {
    Ok(stream) => { /* success */ },
    Err(BuildStreamError::StreamConfigNotSupported) => {
        config.buffer_size = BufferSize::Default;  // Fallback
        device.build_output_stream(&config, data_fn, err_fn, None)?
    },
    Err(e) => return Err(e),
}
```

**JACK users:** Use `BufferSize::Default` to automatically match the server's configured size.

## 5. Dependency updates

Update these dependencies if you use them directly:

```toml
[dependencies]
cpal = "0.17"

# Platform-specific (if used directly):
alsa = "0.11"  # Linux only
windows = { version = ">=0.59, <=0.62" }  # Windows only
audio_thread_priority = "0.34"  # All platforms
```

## 6. ALSA device enumeration changed (Linux)

**What changed:** Device enumeration now returns all devices from `aplay -L`. v0.16 had a regression that only returned card names, missing all device variants.

* v0.16: Only card names ("Loopback", "HDA Intel PCH")
* v0.17: All aplay -L devices (default, hw:CARD=X,DEV=Y, plughw:, front:, surround51:, etc.)

**Impact:** Many more devices will be enumerated. Device names/IDs will be much more detailed. Update any code that matches specific ALSA device names.

## 7. Wasm wee_alloc feature removed

**What changed:** The optional `wee_alloc` feature was removed for security reasons.

```toml
# Before (v0.16)
cpal = { version = "0.16", features = ["wasm-bindgen", "wee_alloc"] }

# After (v0.17)
cpal = { version = "0.17", features = ["wasm-bindgen"] }
```

## Notable Non-Breaking Improvements

v0.17 also includes significant improvements that don't require code changes:

- **Stable device IDs:** New `device.id()` returns persistent device identifiers that survive reboots/reconnections. Use `host.device_by_id()` to reliably select saved devices.
- **Streams are Send+Sync everywhere:** All platforms now support moving/sharing streams across threads
- **24-bit sample formats:** Added `I24`/`U24` support on ALSA, CoreAudio, WASAPI, ASIO
- **Custom host support:** Implement your own `Host`/`Device`/`Stream` for proprietary platforms
- **Predictable buffer sizes:** CoreAudio and AAudio now ensure consistent callback buffer sizes
- **Expanded sample rate support:** ALSA supports 12, 24, 352.8, 384, 705.6, and 768 kHz
- **WASAPI advanced interop:** Exposed `IMMDevice` for Windows COM interop scenarios
- **Platform improvements:** macOS loopback recording (14.6+), improved ALSA audio callback performance, improved timestamp accuracy, iOS AVAudioSession integration, JACK on all platforms

See [CHANGELOG.md](CHANGELOG.md) for complete details and [examples/](examples/) for updated usage patterns.

---

## Getting Help

- Full details: [CHANGELOG.md](CHANGELOG.md)
- Examples: [examples/](examples/)
- Issues: https://github.com/RustAudio/cpal/issues

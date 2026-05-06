# Upgrading from v0.17 to v0.18

This guide covers breaking changes requiring code updates. See [CHANGELOG.md](CHANGELOG.md) for the complete list of changes and improvements.

## Breaking Changes Checklist

- [ ] Replace per-operation error type matches with `e.kind()` on `ErrorKind`
- [ ] Replace `HostUnavailable` error type with `ErrorKind::HostUnavailable` on `Error`
- [ ] Change `build_*_stream` call sites to pass `StreamConfig` by value (drop the `&`)
- [ ] For custom hosts, change `DeviceTrait` implementations to accept `StreamConfig` by value.
- [ ] Remove `instant.duration_since(e)` unwraps; it now returns `Duration` (saturating).
- [ ] Change `instant.add(d)` to `instant.checked_add(d)` (or use `instant + d`).
- [ ] Change `instant.sub(d)` to `instant.checked_sub(d)` (or use `instant - d`).
- [ ] Update `StreamInstant::new(secs, nanos)` call sites: `secs` is now `u64`.
- [ ] Update `StreamInstant::from_nanos(nanos)` call sites: `nanos` is now `u64`.
- [ ] Update `duration_since` call sites to pass by value (drop the `&`).
- [ ] Migrate `wasm32-unknown-emscripten` to `wasm32-unknown-unknown` if possible.
- [ ] Raise your `windows` dependency to `>= 0.61` if you pin it below that.
- [ ] If you relied on the default config returning 44.1 kHz, pin the sample rate explicitly.
- [ ] If you relied on the default config returning `F32`, pin the sample format explicitly.
- [ ] **JACK**: Handle or discard the new `Result` from `Stream::connect_to_system_outputs()` and
  `Stream::connect_to_system_inputs()`.

## 1. Unified `Error` and `ErrorKind` type

**What changed:** All per-operation error types (`DevicesError`, `SupportedStreamConfigsError`,
`DefaultStreamConfigError`, `BuildStreamError`, `StreamError`, `PlayStreamError`,
`PauseStreamError`) and the `HostUnavailable` struct are replaced by a single `cpal::Error` struct
with getters for its `kind()` and optional `message()`.

```rust
// Before (v0.17): each operation returned its own error type
match device.default_output_config() {
    Ok(config) => config,
    Err(DefaultStreamConfigError::DeviceNotAvailable) => panic!("device gone"),
    Err(DefaultStreamConfigError::StreamTypeNotSupported) => panic!("unsupported"),
    Err(DefaultStreamConfigError::BackendSpecific { err }) => panic!("{err}"),
}

// After (v0.18): all operations return cpal::Error; match on e.kind()
match device.default_output_config() {
    Ok(config) => config,
    Err(e) => match e.kind() {
        cpal::ErrorKind::DeviceNotAvailable => panic!("device gone"),
        cpal::ErrorKind::UnsupportedConfig => panic!("unsupported"),
        cpal::ErrorKind::DeviceBusy => {
            std::thread::sleep(std::time::Duration::from_millis(100));
            // retry
        }
        _ => panic!("{e}"),
    },
}
```

The `ErrorKind` variants and their equivalents from v0.17:

| `ErrorKind`            | Former equivalent                                    |
|------------------------|------------------------------------------------------|
| `HostUnavailable`      | `HostUnavailable` (struct)                           |
| `DeviceNotAvailable`   | `DeviceNotAvailable` in most enums                   |
| `DeviceBusy`           | - (new; previously mapped to `DeviceNotAvailable`)   |
| `UnsupportedConfig`    | `StreamConfigNotSupported`, `StreamTypeNotSupported` |
| `UnsupportedOperation` | - (new)                                              |
| `InvalidInput`         | - (new)                                              |
| `StreamInvalidated`    | `StreamError::StreamInvalidated`                     |
| `Xrun`                 | `StreamError::BufferUnderrun`                        |
| `PermissionDenied`     | - (new)                                              |
| `Other`                | `BackendSpecific`                                    |

The `message()` getter on `Error` returns human-readable context (formerly in
`BackendSpecific::err`).

**Why:** A single type simplifies error handling across all cpal operations and allows new
`ErrorKind` variants to be added without changing any return types.

## 2. `StreamConfig` is now passed by value

**What changed:** `StreamConfig` now implements `Copy`, and all `DeviceTrait` stream-building methods accept it by value.

```rust
// Before (v0.17)
let stream = device.build_output_stream(&config, data_fn, err_fn, None)?;

// After (v0.18)
let stream = device.build_output_stream(config, data_fn, err_fn, None)?;
```

**Impact:** Remove the `&` at every `build_*_stream` call site. Because `StreamConfig` is `Copy`, you can reuse the same binding across multiple calls without cloning.

If you implement `DeviceTrait` on your own type (via the `custom` feature), update your `build_input_stream_raw` and `build_output_stream_raw` signatures from `config: &StreamConfig` to `config: StreamConfig`. Any `config.clone()` calls before `move` closures can also be removed.

## 3. `StreamInstant` API overhaul

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

## 4. Default sample rate changed to 48 kHz

**What changed:** `default_input_config()` and `default_output_config()` now prefer 48 kHz over
44.1 kHz on backends that apply a heuristic. The new selection order is 48 kHz, then 44.1 kHz, then the device maximum.

If your pipeline requires 44.1 kHz, request it explicitly:

```rust
let config = device
    .supported_output_configs()?
    .find_map(|r| r.try_with_sample_rate(cpal::SAMPLE_RATE_CD))
    .expect("device does not support 44.1 kHz");
```

**Why:** 48 kHz is the native rate of virtually all modern hardware; the old default caused
unnecessary resampling on such devices.

## 5. Default sample format selection changed

**What changed:** `default_input_config()` and `default_output_config()` now use a fully-ranked
format ordering across all `SampleFormat` variants.

Previously only `F32`, `I16`, and `U16` were explicitly ranked; all other formats compared as
equal. On hosts that expose higher-precision formats, the default config may now return a
different format than before. Likely candidates are `I32`, `I24`, or even `F64` if the device 
or plugin supports it.

If your code assumes the default format is `F32`, request the format explicitly:

```rust
// Request F32 explicitly instead of relying on the default
let configs = device
    .supported_output_configs()?
    .filter(|r| r.sample_format() == SampleFormat::F32);
```

**Why:** The previous heuristic only explicitly ranked three formats, making selection
unpredictable for any other format the device reported. The new ordering is complete and
consistent: floats before integers (F64 > F32 for maximum fidelity), integers by bit-depth
descending with signed above unsigned at each width, and DSD last.

## 6. `audio_thread_priority` feature renamed to `realtime-dbus` and enabled by default

**What changed:** The `audio_thread_priority` feature has been renamed to `realtime-dbus` and is
now a default feature. If you did not previously enable it, real-time scheduling will now be
requested automatically for audio callback threads. A new `realtime` feature was also added,
providing the same scheduling promotion without a D-Bus build dependency.

```toml
# Before (v0.17): opt-in required
cpal = { version = "0.17", features = ["audio_thread_priority"] }

# After (v0.18): on by default; rename the feature if you were opting in
cpal = { version = "0.18" }

# To opt out explicitly:
cpal = { version = "0.18", default-features = false }
```

On Linux and BSD, `realtime-dbus` requires `libdbus-1-dev` (Debian/Ubuntu), `dbus-devel`
(Fedora/RHEL), or equivalent at build time. On headless or embedded targets without D-Bus, use
`realtime` instead:

```toml
cpal = { version = "0.18", default-features = false, features = ["realtime"] }
```

For both features, promotion failures are non-fatal: the stream still starts and an
`ErrorKind::RealtimeDenied` error is delivered through `error_callback`.

**Impact:** In most cases no action is needed. If your `Cargo.toml` names `audio_thread_priority`
explicitly, rename it to `realtime-dbus`. If you relied on the opt-out behavior, pass
`default-features = false`.

**Why:** Real-time scheduling is the correct default for audio applications. The previous opt-in
made it easy to accidentally ship without it.

## 7. `wasm32-unknown-emscripten` target removed

**What changed:** The `emscripten` audio host and the `wasm32-unknown-emscripten` build target are no longer supported.

Migrate to `wasm32-unknown-unknown` and enable the `wasm-bindgen` feature:

```toml
# Before (v0.17)
cpal = { version = "0.17", features = ["emscripten"] }

# After (v0.18)
cpal = { version = "0.18", features = ["wasm-bindgen"] }
```

Then select the `webaudio` host at runtime:

```rust
let host = cpal::host_from_id(cpal::HostId::WebAudio)?;
```

If you must target `wasm32-unknown-emscripten` specifically, consider using OpenAL or another audio approach that supports that target, as cpal no longer provides audio on Emscripten.

**Why:** The old `emscripten` host relied on deprecated Emscripten audio APIs that are no longer functional.

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

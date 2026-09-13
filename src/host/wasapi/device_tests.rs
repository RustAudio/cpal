use super::*;

fn assert_exclusive_rejected<T>(result: Result<T, crate::Error>) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("exclusive WASAPI operation unexpectedly succeeded"),
    };
    assert_eq!(error.kind(), crate::ErrorKind::UnsupportedOperation);
    assert_eq!(
        error.message(),
        Some("Exclusive WASAPI input and loopback are not supported")
    );
}

#[test]
fn exclusive_input_rejects_before_validation_or_activation() {
    use crate::traits::DeviceTrait;

    let device = Device::default_input().exclusive(true);
    assert!(!device.supports_input());

    // Zero channels/rate is deliberately invalid: check_mode must win before validation or any
    // endpoint activation. The default-input constructor itself only stores a virtual handle.
    let invalid = StreamConfig {
        channels: 0,
        sample_rate: 0,
        buffer_size: BufferSize::Default,
    };
    assert_exclusive_rejected(device.default_input_config());
    assert_exclusive_rejected(device.supported_input_configs());
    assert_exclusive_rejected(device.build_input_stream_raw(
        invalid,
        SampleFormat::F32,
        |_, _| {},
        |_| {},
        None,
    ));
    assert_exclusive_rejected(device.build_input_stream::<f32, _, _>(
        invalid,
        |_, _| {},
        |_| {},
        None,
    ));
}

#[test]
fn exclusive_render_rejects_input_loopback_before_activation() {
    use crate::traits::DeviceTrait;

    let device = Device::default_output().exclusive(true);
    assert!(!device.supports_input());
    let invalid = StreamConfig {
        channels: 0,
        sample_rate: 0,
        buffer_size: BufferSize::Default,
    };
    assert_exclusive_rejected(device.build_input_stream_raw(
        invalid,
        SampleFormat::F32,
        |_, _| {},
        |_| {},
        None,
    ));
}

#[test]
fn facade_exclusive_input_rejects_before_validation_or_activation() {
    use crate::platform::Device as PlatformDevice;
    use crate::traits::DeviceTrait;

    let device: PlatformDevice = Device::default_input().into();
    let exclusive = device.exclusive(true);
    assert!(!exclusive.supports_input());
    let invalid = StreamConfig {
        channels: 0,
        sample_rate: 0,
        buffer_size: BufferSize::Default,
    };
    assert_exclusive_rejected(exclusive.default_input_config());
    assert_exclusive_rejected(exclusive.supported_input_configs());
    assert_exclusive_rejected(exclusive.build_input_stream_raw(
        invalid,
        SampleFormat::F32,
        |_, _| {},
        |_| {},
        None,
    ));
    assert_exclusive_rejected(exclusive.build_input_stream::<f32, _, _>(
        invalid,
        |_, _| {},
        |_| {},
        None,
    ));
}

#[test]
fn facade_exclusive_render_rejects_loopback_input() {
    use crate::platform::Device as PlatformDevice;
    use crate::traits::DeviceTrait;

    let device: PlatformDevice = Device::default_output().into();
    let exclusive = device.exclusive(true);
    assert!(!exclusive.supports_input());
    let invalid = StreamConfig {
        channels: 0,
        sample_rate: 0,
        buffer_size: BufferSize::Default,
    };
    assert_exclusive_rejected(exclusive.build_input_stream_raw(
        invalid,
        SampleFormat::F32,
        |_, _| {},
        |_| {},
        None,
    ));
}

#[test]
fn wrapping_native_mode_preserves_exclusive_request() {
    use crate::platform::{Device as PlatformDevice, DeviceInner};
    let native = Device::default_output().exclusive(true);
    let wrapped = PlatformDevice::from(native.clone());
    assert!(!wrapped.supports_input());
    let unwrap = |device: PlatformDevice| match device.into_inner() {
        DeviceInner::Wasapi(device) => device,
        #[allow(unreachable_patterns)]
        _ => panic!("wrong backend"),
    };
    assert!(unwrap(wrapped.clone()).is_exclusive());
    assert!(unwrap(PlatformDevice::from(wrapped.into_inner())).is_exclusive());
    assert!(unwrap(PlatformDevice::from(DeviceInner::Wasapi(native))).is_exclusive());
    let mut replaced = PlatformDevice::from(Device::default_output()).exclusive(true);
    *replaced.as_inner_mut() = DeviceInner::Wasapi(Device::default_output());
    assert!(unwrap(replaced.clone()).is_exclusive());
    assert!(!unwrap(replaced.exclusive(false)).is_exclusive());
    let mut shared = PlatformDevice::from(Device::default_output());
    *shared.as_inner_mut() = DeviceInner::Wasapi(Device::default_output().exclusive(true));
    assert!(unwrap(shared.clone()).is_exclusive());
    assert!(!unwrap(shared.exclusive(false)).is_exclusive());
}

#[test]
fn frame_duration_round_trips() {
    for rate in [44_100, 48_000, 96_000] {
        for frames in [128, 256, 512, 1024] {
            let duration = buffer_size_to_duration(&BufferSize::Fixed(frames), rate);
            assert!(duration > 0);
            assert_eq!(buffer_duration_to_frames(duration, rate), frames);
        }
    }
}

#[test]
fn i24_uses_a_four_byte_container() {
    let config = StreamConfig {
        channels: 2,
        sample_rate: 48_000,
        buffer_size: BufferSize::Default,
    };
    let wave = config_to_waveformatextensible(config, SampleFormat::I24, Some(3)).unwrap();
    let bits = wave.Format.wBitsPerSample;
    let align = wave.Format.nBlockAlign;
    let valid = unsafe { wave.Samples.wValidBitsPerSample };
    assert_eq!(bits, 32);
    assert_eq!(valid, 24);
    assert_eq!(align, 8);
    assert!(!WAVEFORMATEXTENSIBLE_SAMPLE_FORMATS.contains(&SampleFormat::U16));
}

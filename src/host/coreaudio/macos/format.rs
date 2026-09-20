//! Negotiate the device's physical stream format and nominal sample rate.
use std::{
    mem::{self, size_of},
    ptr::{NonNull, null},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::Instant,
};

use coreaudio::audio_unit::{
    SampleFormat as CoreAudioSampleFormat, StreamFormat, audio_format::LinearPcmFlags,
    macos_helpers::find_matching_physical_format,
};
use objc2_core_audio::{
    AudioDeviceID, AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertyScope,
    AudioObjectSetPropertyData, AudioStreamID, kAudioDevicePropertyAvailableNominalSampleRates,
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertyStreams, kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioStreamPropertyPhysicalFormat,
};
use objc2_core_audio_types::{AudioStreamBasicDescription, AudioValueRange};

use super::{
    property::{get_property, get_property_array},
    property_listener::AudioObjectPropertyListener,
};
use crate::{ChannelCount, Error, ErrorKind, SampleFormat, SampleRate};

const PHYSICAL_FORMAT_ADDRESS: AudioObjectPropertyAddress = AudioObjectPropertyAddress {
    mSelector: kAudioStreamPropertyPhysicalFormat,
    mScope: kAudioObjectPropertyScopeGlobal,
    mElement: kAudioObjectPropertyElementMain,
};

const NOMINAL_SAMPLE_RATE_ADDRESS: AudioObjectPropertyAddress = AudioObjectPropertyAddress {
    mSelector: kAudioDevicePropertyNominalSampleRate,
    mScope: kAudioObjectPropertyScopeGlobal,
    mElement: kAudioObjectPropertyElementMain,
};

const DEVICE_IS_ALIVE_ADDRESS: AudioObjectPropertyAddress = AudioObjectPropertyAddress {
    mSelector: kAudioDevicePropertyDeviceIsAlive,
    mScope: kAudioObjectPropertyScopeGlobal,
    mElement: kAudioObjectPropertyElementMain,
};

/// Resolve the first `AudioStreamID` a device exposes in `scope` (Input or Output).
///
/// `kAudioStreamPropertyPhysicalFormat` belongs to the stream object, not the device: reads
/// against the device happen to be forwarded by the HAL, but property-changed notifications are
/// not, so listeners must be registered on the stream object directly.
fn first_stream_id(
    device_id: AudioDeviceID,
    scope: AudioObjectPropertyScope,
) -> Result<AudioStreamID, coreaudio::Error> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreams,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    };
    // SAFETY: kAudioDevicePropertyStreams is documented to return an array of AudioStreamID.
    let stream_ids: Vec<AudioStreamID> = unsafe { get_property_array(device_id, address) }?;
    stream_ids
        .into_iter()
        .next()
        .ok_or(coreaudio::Error::UnsupportedStreamFormat)
}

fn physical_format(
    stream_id: AudioStreamID,
) -> Result<AudioStreamBasicDescription, coreaudio::Error> {
    // SAFETY: kAudioStreamPropertyPhysicalFormat is documented to return an AudioStreamBasicDescription.
    unsafe { get_property(stream_id, PHYSICAL_FORMAT_ADDRESS) }
}

fn asbds_are_equal(
    left: &AudioStreamBasicDescription,
    right: &AudioStreamBasicDescription,
) -> bool {
    left.mSampleRate as u32 == right.mSampleRate as u32
        && left.mFormatID == right.mFormatID
        && left.mFormatFlags == right.mFormatFlags
        && left.mBytesPerPacket == right.mBytesPerPacket
        && left.mFramesPerPacket == right.mFramesPerPacket
        && left.mBytesPerFrame == right.mBytesPerFrame
        && left.mChannelsPerFrame == right.mChannelsPerFrame
        && left.mBitsPerChannel == right.mBitsPerChannel
}

/// Set the device's physical stream format and wait until it reports the new one.
///
/// Gives up at `deadline`, or waits indefinitely if it is `None`. If the device disconnects, the
/// wait ends early.
fn set_physical_stream_format(
    device_id: AudioDeviceID,
    scope: AudioObjectPropertyScope,
    new_asbd: AudioStreamBasicDescription,
    deadline: Option<Instant>,
) -> Result<(), Error> {
    let stream_id = first_stream_id(device_id, scope)?;
    if asbds_are_equal(&physical_format(stream_id)?, &new_asbd) {
        return Ok(());
    }

    // Listen before setting the format, so the change can't be missed.
    let (receiver, _listeners) = watch_property(
        device_id,
        stream_id,
        PHYSICAL_FORMAT_ADDRESS,
        physical_format,
    )?;

    let address = PHYSICAL_FORMAT_ADDRESS;
    let status = unsafe {
        AudioObjectSetPropertyData(
            stream_id,
            NonNull::from(&address),
            0,
            null(),
            size_of::<AudioStreamBasicDescription>() as u32,
            NonNull::from(&new_asbd).cast(),
        )
    };
    coreaudio::Error::from_os_status(status)?;

    wait_for_property(&receiver, deadline, "physical format", |asbd| {
        asbds_are_equal(asbd, &new_asbd)
    })
}

/// Try to find a matching physical stream format on the device and apply it.
///
/// This makes the hardware run at the requested bit depth and sample rate directly, without
/// unnecessary conversions.
pub fn set_physical_format(
    device_id: AudioDeviceID,
    scope: AudioObjectPropertyScope,
    sample_rate: SampleRate,
    channels: ChannelCount,
    sample_format: SampleFormat,
    deadline: Option<Instant>,
) -> Result<AudioStreamBasicDescription, Error> {
    let core_format = match sample_format {
        SampleFormat::I8 => CoreAudioSampleFormat::I8,
        SampleFormat::I16 => CoreAudioSampleFormat::I16,
        SampleFormat::I24 => CoreAudioSampleFormat::I24,
        SampleFormat::I32 => CoreAudioSampleFormat::I32,
        SampleFormat::F32 => CoreAudioSampleFormat::F32,
        _ => return Err(coreaudio::Error::UnsupportedStreamFormat.into()),
    };
    let stream_format = StreamFormat {
        sample_rate: sample_rate as f64,
        sample_format: core_format,
        flags: LinearPcmFlags::empty(),
        channels: channels as u32,
    };
    let asbd = find_matching_physical_format(device_id, stream_format)
        .ok_or(coreaudio::Error::UnsupportedStreamFormat)?;
    set_physical_stream_format(device_id, scope, asbd, deadline).map(|_| asbd)
}

/// Read the device's current nominal sample rate.
///
/// "Nominal" is CoreAudio's term for the rate the device is configured to run at, as opposed to
/// the actual rate measured from its hardware clock (`kAudioDevicePropertyActualSampleRate`).
fn nominal_sample_rate(audio_device_id: AudioObjectID) -> Result<f64, coreaudio::Error> {
    // SAFETY: kAudioDevicePropertyNominalSampleRate is documented to return an f64.
    unsafe { get_property(audio_device_id, NOMINAL_SAMPLE_RATE_ADDRESS) }
}

/// Set the device's nominal sample rate via `kAudioDevicePropertyNominalSampleRate`.
///
/// Unlike [`set_physical_format`], this only changes the device clock rate. The AudioUnit bridges
/// any remaining format difference to the virtual stream format the callback sees.
pub fn set_sample_rate(
    audio_device_id: AudioObjectID,
    target_sample_rate: SampleRate,
    deadline: Option<Instant>,
) -> Result<(), Error> {
    let sample_rate = nominal_sample_rate(audio_device_id)?;
    let mut property_address = NOMINAL_SAMPLE_RATE_ADDRESS;

    // If the requested sample rate is different to the device sample rate, update the device.
    if (sample_rate - target_sample_rate as f64).abs() >= 1.0 {
        // Get available sample rate ranges.
        property_address.mSelector = kAudioDevicePropertyAvailableNominalSampleRates;
        // SAFETY: kAudioDevicePropertyAvailableNominalSampleRates is documented to return an
        // array of AudioValueRange.
        let ranges: Vec<AudioValueRange> =
            unsafe { get_property_array(audio_device_id, property_address) }?;

        // Now that we have the available ranges, pick the one matching the desired rate.
        let sample_rate = target_sample_rate;
        if !ranges
            .iter()
            .any(|r| sample_rate as f64 >= r.mMinimum && sample_rate as f64 <= r.mMaximum)
        {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!("Sample rate {sample_rate} Hz is not supported"),
            ));
        }

        // Listen before setting the rate, so that neither the new rate nor a disconnect is missed.
        let (receiver, _listeners) = watch_property(
            audio_device_id,
            audio_device_id,
            NOMINAL_SAMPLE_RATE_ADDRESS,
            nominal_sample_rate,
        )?;

        // Set the nominal sample rate.
        property_address.mSelector = kAudioDevicePropertyNominalSampleRate;
        let rate = sample_rate as f64;
        let data_size = mem::size_of::<f64>() as u32;
        let status = unsafe {
            AudioObjectSetPropertyData(
                audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                data_size,
                NonNull::from(&rate).cast(),
            )
        };
        coreaudio::Error::from_os_status(status)?;

        // Wait for the reported_rate to change. This should not take longer than a few ms.
        wait_for_rate(&receiver, target_sample_rate, deadline)?;
        // listeners are removed when they drop here
    }
    Ok(())
}

/// What a device reports while one of its properties is changing.
enum PropertyEvent<T> {
    /// The property now has this value.
    Changed(T),
    /// The device disconnected, or can no longer be queried.
    Unavailable,
}

/// Report a property change, or a device disconnect, as a [`PropertyEvent`].
///
/// The listeners stay registered for as long as the returned guards are alive.
/// `alive_id` is the device to watch for a disconnect. `target_id` is the object that actually
/// owns `address`: the device itself, or one of its streams.
fn watch_property<T: Send + 'static>(
    alive_id: AudioDeviceID,
    target_id: AudioObjectID,
    address: AudioObjectPropertyAddress,
    read: fn(AudioObjectID) -> Result<T, coreaudio::Error>,
) -> Result<(Receiver<PropertyEvent<T>>, [AudioObjectPropertyListener; 2]), Error> {
    let (sender, receiver) = channel();
    let alive_sender = sender.clone();
    let alive = AudioObjectPropertyListener::new(alive_id, DEVICE_IS_ALIVE_ADDRESS, move || {
        let _ = alive_sender.send(PropertyEvent::Unavailable);
    })?;
    let changed = AudioObjectPropertyListener::new(target_id, address, move || {
        let event = read(target_id).map_or(PropertyEvent::Unavailable, PropertyEvent::Changed);
        let _ = sender.send(event);
    })?;
    Ok((receiver, [alive, changed]))
}

/// Block until a reported value satisfies `is_target`, giving up at `deadline`.
///
/// Other values can be reported first, so `deadline` bounds the whole wait rather than each
/// individual receive. A `deadline` of `None` waits indefinitely, ending early only if the device
/// disappears.
fn wait_for_property<T>(
    receiver: &Receiver<PropertyEvent<T>>,
    deadline: Option<Instant>,
    what: &str,
    mut is_target: impl FnMut(&T) -> bool,
) -> Result<(), Error> {
    let timed_out = || {
        Error::with_message(
            ErrorKind::DeviceNotAvailable,
            format!("Timed out waiting for the {what} to change"),
        )
    };

    loop {
        let received = match deadline {
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(timed_out());
                }
                receiver.recv_timeout(remaining)
            }
            None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };

        match received {
            Ok(PropertyEvent::Changed(value)) => {
                if is_target(&value) {
                    return Ok(());
                }
            }
            Ok(PropertyEvent::Unavailable) => {
                return Err(Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    format!("Device disconnected while updating the {what}"),
                ));
            }
            Err(RecvTimeoutError::Timeout) => return Err(timed_out()),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Error::with_message(
                    ErrorKind::StreamInvalidated,
                    format!("Listener for the {what} disconnected unexpectedly"),
                ));
            }
        }
    }
}

/// Block until the device reports `target_sample_rate`, giving up at `deadline`.
fn wait_for_rate(
    receiver: &Receiver<PropertyEvent<f64>>,
    target_sample_rate: SampleRate,
    deadline: Option<Instant>,
) -> Result<(), Error> {
    wait_for_property(receiver, deadline, "sample rate", |rate| {
        (rate - target_sample_rate as f64).abs() < 1.0
    })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    use super::{PropertyEvent, wait_for_rate};

    /// A listener can report rates other than the target before it reports the new one: for
    /// example, the device might step through several rates first. The whole timeout must remain
    /// available across those.
    #[test]
    fn wait_for_rate_honours_the_full_timeout_across_repeated_events() {
        const TIMEOUT: Duration = Duration::from_millis(50);

        let (sender, receiver) = channel::<PropertyEvent<f64>>();
        let feeder = std::thread::spawn(move || {
            while sender.send(PropertyEvent::Changed(44_100.0)).is_ok() {
                std::thread::sleep(Duration::from_millis(1));
            }
        });

        let start = Instant::now();
        assert!(wait_for_rate(&receiver, 48_000, Some(start + TIMEOUT)).is_err());
        let elapsed = start.elapsed();

        drop(receiver);
        let _ = feeder.join();

        assert!(
            elapsed >= TIMEOUT - Duration::from_millis(10),
            "gave up after {elapsed:?}, well before the {TIMEOUT:?} timeout"
        );
    }

    #[test]
    fn wait_for_rate_returns_when_the_target_rate_is_reported() {
        let (sender, receiver) = channel::<PropertyEvent<f64>>();
        sender.send(PropertyEvent::Changed(44_100.0)).unwrap();
        sender.send(PropertyEvent::Changed(48_000.0)).unwrap();

        assert!(
            wait_for_rate(
                &receiver,
                48_000,
                Some(Instant::now() + Duration::from_secs(5))
            )
            .is_ok()
        );
    }

    /// The wait must end when the device disappears, whether it is unbounded or has a long timeout.
    #[test]
    fn wait_for_rate_ends_early_on_disconnect() {
        for timeout in [None, Some(Duration::from_secs(30))] {
            let (sender, receiver) = channel::<PropertyEvent<f64>>();
            let disconnect = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                sender.send(PropertyEvent::Unavailable).unwrap();
            });

            let start = Instant::now();
            let deadline = timeout.map(|timeout| start + timeout);
            assert!(wait_for_rate(&receiver, 48_000, deadline).is_err());
            let elapsed = start.elapsed();
            disconnect.join().unwrap();

            assert!(
                elapsed < Duration::from_secs(5),
                "waited {elapsed:?} with timeout {timeout:?}"
            );
        }
    }
}

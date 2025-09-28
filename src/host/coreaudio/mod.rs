use objc2_core_audio_types::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatFlagIsSignedInteger,
    kAudioFormatLinearPCM, AudioStreamBasicDescription,
};

use crate::DefaultStreamConfigError;
use crate::{BuildStreamError, SupportedStreamConfigsError};

use crate::{BackendSpecificError, SampleFormat, StreamConfig};

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "ios")]
pub use self::ios::{
    enumerate::{Devices, SupportedInputConfigs, SupportedOutputConfigs},
    Device, Host, Stream,
};

#[cfg(target_os = "macos")]
pub use self::macos::{
    enumerate::{Devices, SupportedInputConfigs, SupportedOutputConfigs},
    Device, Host, Stream,
};

// Common helper methods used by both macOS and iOS

fn check_os_status(os_status: OSStatus) -> Result<(), BackendSpecificError> {
    match coreaudio::Error::from_os_status(os_status) {
        Ok(()) => Ok(()),
        Err(err) => {
            let description = err.to_string();
            Err(BackendSpecificError { description })
        }
    }
}

// Create a coreaudio AudioStreamBasicDescription from a CPAL Format.
fn asbd_from_config(
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> AudioStreamBasicDescription {
    let n_channels = config.channels as usize;
    let sample_rate = config.sample_rate.0;
    let bytes_per_channel = sample_format.sample_size();
    let bits_per_channel = bytes_per_channel * 8;
    let bytes_per_frame = n_channels * bytes_per_channel;
    let frames_per_packet = 1;
    let bytes_per_packet = frames_per_packet * bytes_per_frame;
    let format_flags = match sample_format {
        SampleFormat::F32 | SampleFormat::F64 => kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
        SampleFormat::I8
        | SampleFormat::I16
        | SampleFormat::I24
        | SampleFormat::I32
        | SampleFormat::I64 => kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
        _ => kAudioFormatFlagIsPacked,
    };
    AudioStreamBasicDescription {
        mBitsPerChannel: bits_per_channel as _,
        mBytesPerFrame: bytes_per_frame as _,
        mChannelsPerFrame: n_channels as _,
        mBytesPerPacket: bytes_per_packet as _,
        mFramesPerPacket: frames_per_packet as _,
        mFormatFlags: format_flags,
        mFormatID: kAudioFormatLinearPCM,
        mSampleRate: sample_rate as _,
        mReserved: 0,
    }
}

#[inline]
fn host_time_to_stream_instant(
    m_host_time: u64,
) -> Result<crate::StreamInstant, BackendSpecificError> {
    let mut info: mach2::mach_time::mach_timebase_info = Default::default();
    let res = unsafe { mach2::mach_time::mach_timebase_info(&mut info) };
    check_os_status(res)?;
    let nanos = m_host_time * info.numer as u64 / info.denom as u64;
    let secs = nanos / 1_000_000_000;
    let subsec_nanos = nanos - secs * 1_000_000_000;
    Ok(crate::StreamInstant::new(secs as i64, subsec_nanos as u32))
}

// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
#[inline]
fn frames_to_duration(frames: usize, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate.0 as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

// TODO need stronger error identification
impl From<coreaudio::Error> for BuildStreamError {
    fn from(err: coreaudio::Error) -> BuildStreamError {
        match err {
            coreaudio::Error::RenderCallbackBufferFormatDoesNotMatchAudioUnitStreamFormat
            | coreaudio::Error::NoKnownSubtype
            | coreaudio::Error::AudioUnit(coreaudio::error::AudioUnitError::FormatNotSupported)
            | coreaudio::Error::AudioCodec(_)
            | coreaudio::Error::AudioFormat(_) => BuildStreamError::StreamConfigNotSupported,
            _ => BuildStreamError::DeviceNotAvailable,
        }
    }
}

impl From<coreaudio::Error> for SupportedStreamConfigsError {
    fn from(err: coreaudio::Error) -> SupportedStreamConfigsError {
        let description = format!("{err}");
        let err = BackendSpecificError { description };
        // Check for possible DeviceNotAvailable variant
        SupportedStreamConfigsError::BackendSpecific { err }
    }
}

impl From<coreaudio::Error> for DefaultStreamConfigError {
    fn from(err: coreaudio::Error) -> DefaultStreamConfigError {
        let description = format!("{err}");
        let err = BackendSpecificError { description };
        // Check for possible DeviceNotAvailable variant
        DefaultStreamConfigError::BackendSpecific { err }
    }
}

pub(crate) type OSStatus = i32;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        default_host,
        traits::{DeviceTrait, HostTrait, StreamTrait},
        Sample,
    };

    #[test]
    fn test_stream_thread_transfer() {
        let host = default_host();
        let device = host.default_output_device().unwrap();

        let mut supported_configs_range = device.supported_output_configs().unwrap();
        let supported_config = supported_configs_range
            .next()
            .unwrap()
            .with_max_sample_rate();
        let config = supported_config.config();

        let stream = device
            .build_output_stream(
                &config,
                write_silence::<f32>,
                move |err| println!("Error: {err}"),
                None,
            )
            .unwrap();

        // Move stream to another thread and back - this should compile and work
        let handle = std::thread::spawn(move || {
            // Stream is now owned by this thread
            stream.play().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(100));
            stream.pause().unwrap();
            stream // Return stream back to main thread
        });

        let stream = handle.join().unwrap();
        // Stream is back in main thread
        stream.play().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        stream.pause().unwrap();
    }

    fn write_silence<T: Sample>(data: &mut [T], _: &crate::OutputCallbackInfo) {
        for sample in data.iter_mut() {
            *sample = Sample::EQUILIBRIUM;
        }
    }
}

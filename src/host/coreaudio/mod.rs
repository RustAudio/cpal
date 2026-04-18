//! CoreAudio backend implementation.
//!
//! Default backend on macOS, iOS, and tvOS.

use objc2_core_audio_types::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatFlagIsSignedInteger,
    kAudioFormatLinearPCM, AudioStreamBasicDescription,
};

use crate::{Error, ErrorKind, SampleFormat, StreamConfig};

// iOS and tvOS share the same CoreAudio / AudioUnit surface (RemoteIO,
// AVAudioSession), so both target the `ios` submodule.
#[cfg(not(target_os = "macos"))]
mod ios;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
#[allow(unused_imports)]
pub use self::ios::{
    enumerate::{Devices, SupportedInputConfigs, SupportedOutputConfigs},
    Device, Host, Stream,
};

#[cfg(target_os = "macos")]
pub use self::macos::{Host, Stream};

// Common helper methods used by both macOS and iOS

fn check_os_status(os_status: OSStatus) -> Result<(), Error> {
    coreaudio::Error::from_os_status(os_status).map_err(Error::from)
}

// Create a coreaudio AudioStreamBasicDescription from a CPAL Format.
fn asbd_from_config(
    config: StreamConfig,
    sample_format: SampleFormat,
) -> AudioStreamBasicDescription {
    let n_channels = config.channels as usize;
    let sample_rate = config.sample_rate;
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
fn host_time_to_stream_instant(m_host_time: u64) -> Result<crate::StreamInstant, Error> {
    let mut info: mach2::mach_time::mach_timebase_info = Default::default();
    let res = unsafe { mach2::mach_time::mach_timebase_info(&mut info) };
    check_os_status(res)?;
    let nanos = m_host_time as u128 * info.numer as u128 / info.denom as u128;
    let secs = u64::try_from(nanos / 1_000_000_000)
        .map_err(|_| Error::with_message(ErrorKind::Other, "mach absolute time overflow"))?;
    let subsec_nanos = (nanos % 1_000_000_000) as u32;
    Ok(crate::StreamInstant::new(secs, subsec_nanos))
}

// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
#[inline]
fn frames_to_duration(frames: usize, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

impl From<coreaudio::Error> for Error {
    fn from(err: coreaudio::Error) -> Self {
        use coreaudio::error::{AudioCodecError, AudioError, AudioFormatError, AudioUnitError};
        let msg = format!("{err}");
        match err {
            coreaudio::Error::RenderCallbackBufferFormatDoesNotMatchAudioUnitStreamFormat
            | coreaudio::Error::NoKnownSubtype
            | coreaudio::Error::UnsupportedSampleRate
            | coreaudio::Error::UnsupportedStreamFormat
            | coreaudio::Error::NonInterleavedInputOnlySupportsMono
            | coreaudio::Error::AudioUnit(AudioUnitError::FormatNotSupported)
            | coreaudio::Error::AudioUnit(AudioUnitError::InvalidPropertyValue)
            | coreaudio::Error::AudioUnit(AudioUnitError::TooManyFramesToProcess)
            | coreaudio::Error::AudioCodec(AudioCodecError::UnsupportedFormat)
            | coreaudio::Error::AudioFormat(AudioFormatError::UnsupportedDataFormat)
            | coreaudio::Error::AudioFormat(AudioFormatError::UnknownFormat) => {
                Error::with_message(ErrorKind::UnsupportedConfig, msg)
            }

            coreaudio::Error::SystemSoundClientMessageTimedOut
            | coreaudio::Error::NoMatchingDefaultAudioUnitFound
            | coreaudio::Error::AudioUnit(AudioUnitError::NoConnection)
            | coreaudio::Error::AudioUnit(AudioUnitError::FailedInitialization)
            | coreaudio::Error::Audio(AudioError::FileNotFound) => {
                Error::with_message(ErrorKind::DeviceNotAvailable, msg)
            }

            coreaudio::Error::AudioUnit(AudioUnitError::Unauthorized)
            | coreaudio::Error::Audio(AudioError::FilePermission) => {
                Error::with_message(ErrorKind::PermissionDenied, msg)
            }

            coreaudio::Error::AudioUnit(AudioUnitError::InvalidProperty)
            | coreaudio::Error::AudioUnit(AudioUnitError::InvalidParameter)
            | coreaudio::Error::AudioUnit(AudioUnitError::InvalidElement)
            | coreaudio::Error::AudioUnit(AudioUnitError::InvalidScope)
            | coreaudio::Error::AudioUnit(AudioUnitError::PropertyNotInUse)
            | coreaudio::Error::AudioCodec(AudioCodecError::UnknownProperty)
            | coreaudio::Error::AudioCodec(AudioCodecError::BadPropertySize)
            | coreaudio::Error::AudioFormat(AudioFormatError::UnsupportedProperty)
            | coreaudio::Error::AudioFormat(AudioFormatError::BadPropertySize)
            | coreaudio::Error::AudioFormat(AudioFormatError::BadSpecifierSize)
            | coreaudio::Error::Audio(AudioError::Param)
            | coreaudio::Error::Audio(AudioError::BadFilePath) => {
                Error::with_message(ErrorKind::InvalidInput, msg)
            }

            coreaudio::Error::Audio(AudioError::Unimplemented)
            | coreaudio::Error::AudioUnit(AudioUnitError::PropertyNotWritable)
            | coreaudio::Error::AudioUnit(AudioUnitError::InvalidOfflineRender)
            | coreaudio::Error::AudioCodec(AudioCodecError::IllegalOperation) => {
                Error::with_message(ErrorKind::UnsupportedOperation, msg)
            }

            _ => Error::with_message(ErrorKind::Other, msg),
        }
    }
}

pub(crate) type OSStatus = i32;

// Compile-time assertion that Stream is Send and Sync
crate::assert_stream_send!(Stream);
crate::assert_stream_sync!(Stream);

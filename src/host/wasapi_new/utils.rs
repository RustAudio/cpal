use crate::ErrorKind::{BackendError, UnsupportedOperation};
use crate::{DeviceDirection, SampleFormat, StreamConfig, SupportedBufferSize, SupportedStreamConfig};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::fmt::Debug;
use std::mem::transmute;
use std::ops::{Deref, DerefMut};
use std::slice;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use tap::Pipe;
use wasapi::{Direction, SampleType, WaveFormat};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_EVENT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject};

pub type CpalError     = crate::Error;
pub type WasapiRsError = wasapi::WasapiError;
pub type WindowsError  = windows_core::Error;

impl From<WasapiRsError> for CpalError {
    fn from(value: WasapiRsError) -> Self {
        CpalError::with_message(BackendError, format!("{value}"))
    }
}

impl From<Direction> for DeviceDirection {
    fn from(value: Direction) -> Self {
        match value {
            Direction::Capture => DeviceDirection::Input,
            Direction::Render  => DeviceDirection::Output
        }
    }
}

impl TryFrom<WaveFormat> for SupportedStreamConfig {
    type Error = CpalError;

    fn try_from(waveformat: WaveFormat) -> Result<Self, CpalError> {
        SupportedStreamConfig {
            channels: waveformat.get_nchannels(),
            sample_rate: waveformat.get_samplespersec(),
            buffer_size: SupportedBufferSize::default(),
            sample_format: match waveformat.get_validbitspersample() {
                 8 => SampleFormat::I8,
                16 => SampleFormat::I16,
                24 => SampleFormat::I24,
                32 if waveformat.get_subformat()? == SampleType::Float => SampleFormat::F32,
                32 => SampleFormat::I32,
                _  => return Err(CpalError::with_message(BackendError, "sound server reported unexpected mix format"))
            }
        }
        .pipe(Ok)
    }
}

pub fn validate_direction<const INPUT: bool>(mm_device: &wasapi::Device) -> Result<(), CpalError> {
    let is_input = mm_device.get_direction() == Direction::Capture;
    
    // FIXME: use `bool::ok_or()` once MSRV has risen to 1.98
    if INPUT != is_input {
        return Err(CpalError::with_message(UnsupportedOperation, "This device only operates in the other direction"))
    }
    
    Ok(())
}

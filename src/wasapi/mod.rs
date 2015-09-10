extern crate libc;
extern crate winapi;
extern crate ole32;

use std::io::Error as IoError;
use std::sync::{Arc, Mutex, MutexGuard};
use std::ptr;
use std::mem;

use Format;
use FormatsEnumerationError;
use ChannelPosition;
use SamplesRate;
use SampleFormat;

pub use std::option::IntoIter as OptionIntoIter;
pub use self::enumerate::{EndpointsIterator, get_default_endpoint};
pub use self::voice::{Voice, Buffer};

pub type SupportedFormatsIterator = OptionIntoIter<Format>;

// TODO: these constants should be moved to winapi
const SPEAKER_FRONT_LEFT: winapi::DWORD = 0x1;
const SPEAKER_FRONT_RIGHT: winapi::DWORD = 0x2;
const SPEAKER_FRONT_CENTER: winapi::DWORD = 0x4;
const SPEAKER_LOW_FREQUENCY: winapi::DWORD = 0x8;
const SPEAKER_BACK_LEFT: winapi::DWORD = 0x10;
const SPEAKER_BACK_RIGHT: winapi::DWORD = 0x20;
const SPEAKER_FRONT_LEFT_OF_CENTER: winapi::DWORD = 0x40;
const SPEAKER_FRONT_RIGHT_OF_CENTER: winapi::DWORD = 0x80;
const SPEAKER_BACK_CENTER: winapi::DWORD = 0x100;
const SPEAKER_SIDE_LEFT: winapi::DWORD = 0x200;
const SPEAKER_SIDE_RIGHT: winapi::DWORD = 0x400;
const SPEAKER_TOP_CENTER: winapi::DWORD = 0x800;
const SPEAKER_TOP_FRONT_LEFT: winapi::DWORD = 0x1000;
const SPEAKER_TOP_FRONT_CENTER: winapi::DWORD = 0x2000;
const SPEAKER_TOP_FRONT_RIGHT: winapi::DWORD = 0x4000;
const SPEAKER_TOP_BACK_LEFT: winapi::DWORD = 0x8000;
const SPEAKER_TOP_BACK_CENTER: winapi::DWORD = 0x10000;
const SPEAKER_TOP_BACK_RIGHT: winapi::DWORD = 0x20000;

mod com;
mod enumerate;
mod voice;

fn check_result(result: winapi::HRESULT) -> Result<(), IoError> {
    if result < 0 {
        Err(IoError::from_raw_os_error(result))
    } else {
        Ok(())
    }
}

/// Wrapper because of that stupid decision to remove `Send` and `Sync` from raw pointers.
#[derive(Copy, Clone)]
#[allow(raw_pointer_derive)]
struct IAudioClientWrapper(*mut winapi::IAudioClient);
unsafe impl Send for IAudioClientWrapper {}
unsafe impl Sync for IAudioClientWrapper {}

/// An opaque type that identifies an end point.
pub struct Endpoint {
    device: *mut winapi::IMMDevice,

    /// We cache an uninitialized `IAudioClient` so that we can call functions from it without
    /// having to create/destroy audio clients all the time.
    future_audio_client: Arc<Mutex<Option<IAudioClientWrapper>>>,      // TODO: add NonZero around the ptr
}

unsafe impl Send for Endpoint {}
unsafe impl Sync for Endpoint {}

impl Endpoint {
    #[inline]
    fn from_immdevice(device: *mut winapi::IMMDevice) -> Endpoint {
        Endpoint {
            device: device,
            future_audio_client: Arc::new(Mutex::new(None)),
        }
    }

    /// Ensures that `future_audio_client` contains a `Some` and returns a locked mutex to it.
    fn ensure_future_audio_client(&self) -> Result<MutexGuard<Option<IAudioClientWrapper>>, IoError> {
        let mut lock = self.future_audio_client.lock().unwrap();
        if lock.is_some() {
            return Ok(lock);
        }

        let audio_client: *mut winapi::IAudioClient = unsafe {
            let mut audio_client = mem::uninitialized();
            let hresult = (*self.device).Activate(&winapi::IID_IAudioClient, winapi::CLSCTX_ALL,
                                                  ptr::null_mut(), &mut audio_client);

            // can fail if the device has been disconnected since we enumerated it, or if
            // the device doesn't support playback for some reason
            try!(check_result(hresult));
            assert!(!audio_client.is_null());
            audio_client as *mut _
        };

        *lock = Some(IAudioClientWrapper(audio_client));
        Ok(lock)
    }

    /// Returns an uninitialized `IAudioClient`.
    fn build_audioclient(&self) -> Result<*mut winapi::IAudioClient, IoError> {
        let mut lock = try!(self.ensure_future_audio_client());
        let client = lock.unwrap().0;
        *lock = None;
        Ok(client)
    }

    pub fn get_supported_formats_list(&self)
           -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        // We always create voices in shared mode, therefore all samples go through an audio
        // processor to mix them together.
        // However there is no way to query the list of all formats that are supported by the
        // audio processor, but one format is guaranteed to be supported, the one returned by
        // `GetMixFormat`.

        // initializing COM because we call `CoTaskMemFree`
        com::com_initialized();

        let lock = match self.ensure_future_audio_client() {
            Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                return Err(FormatsEnumerationError::DeviceNotAvailable),
            e => e.unwrap(),
        };
        let client = lock.unwrap().0;

        unsafe {
            let mut format_ptr = mem::uninitialized();
            match check_result((*client).GetMixFormat(&mut format_ptr)) {
                Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => {
                    return Err(FormatsEnumerationError::DeviceNotAvailable);
                },
                Err(e) => panic!("{:?}", e),
                Ok(()) => (),
            };

            let format = {
                let (channels, data_type) = match (*format_ptr).wFormatTag {
                    winapi::WAVE_FORMAT_PCM => {
                        (
                            vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
                            SampleFormat::I16
                        )
                    },
                    winapi::WAVE_FORMAT_EXTENSIBLE => {
                        let format_ptr = format_ptr as *const winapi::WAVEFORMATEXTENSIBLE;

                        let channels = {
                            let mut channels = Vec::new();

                            let mask = (*format_ptr).dwChannelMask;
                            if (mask & SPEAKER_FRONT_LEFT) != 0 { channels.push(ChannelPosition::FrontLeft); }
                            if (mask & SPEAKER_FRONT_RIGHT) != 0 { channels.push(ChannelPosition::FrontRight); }
                            if (mask & SPEAKER_FRONT_CENTER) != 0 { channels.push(ChannelPosition::FrontCenter); }
                            if (mask & SPEAKER_LOW_FREQUENCY) != 0 { channels.push(ChannelPosition::LowFrequency); }
                            if (mask & SPEAKER_BACK_LEFT) != 0 { channels.push(ChannelPosition::BackLeft); }
                            if (mask & SPEAKER_BACK_RIGHT) != 0 { channels.push(ChannelPosition::BackRight); }
                            if (mask & SPEAKER_FRONT_LEFT_OF_CENTER) != 0 { channels.push(ChannelPosition::FrontLeftOfCenter); }
                            if (mask & SPEAKER_FRONT_RIGHT_OF_CENTER) != 0 { channels.push(ChannelPosition::FrontRightOfCenter); }
                            if (mask & SPEAKER_BACK_CENTER) != 0 { channels.push(ChannelPosition::BackCenter); }
                            if (mask & SPEAKER_SIDE_LEFT) != 0 { channels.push(ChannelPosition::SideLeft); }
                            if (mask & SPEAKER_SIDE_RIGHT) != 0 { channels.push(ChannelPosition::SideRight); }
                            if (mask & SPEAKER_TOP_CENTER) != 0 { channels.push(ChannelPosition::TopCenter); }
                            if (mask & SPEAKER_TOP_FRONT_LEFT) != 0 { channels.push(ChannelPosition::TopFrontLeft); }
                            if (mask & SPEAKER_TOP_FRONT_CENTER) != 0 { channels.push(ChannelPosition::TopFrontCenter); }
                            if (mask & SPEAKER_TOP_FRONT_RIGHT) != 0 { channels.push(ChannelPosition::TopFrontRight); }
                            if (mask & SPEAKER_TOP_BACK_LEFT) != 0 { channels.push(ChannelPosition::TopBackLeft); }
                            if (mask & SPEAKER_TOP_BACK_CENTER) != 0 { channels.push(ChannelPosition::TopBackCenter); }
                            if (mask & SPEAKER_TOP_BACK_RIGHT) != 0 { channels.push(ChannelPosition::TopBackRight); }

                            assert_eq!((*format_ptr).Format.nChannels as usize, channels.len());
                            channels
                        };

                        let format = match (*format_ptr).SubFormat {
                            winapi::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT => SampleFormat::F32,
                            winapi::KSDATAFORMAT_SUBTYPE_PCM => SampleFormat::I16,
                            g => panic!("Unknown SubFormat GUID returned by GetMixFormat: {:?}", g)
                        };

                        (channels, format)
                    },

                    f => panic!("Unknown data format returned by GetMixFormat: {:?}", f)
                };

                Format {
                    channels: channels,
                    samples_rate: SamplesRate((*format_ptr).nSamplesPerSec),
                    data_type: data_type,
                }
            };

            ole32::CoTaskMemFree(format_ptr as *mut _);

            Ok(Some(format).into_iter())
        }
    }
}

impl PartialEq for Endpoint {
    fn eq(&self, other: &Endpoint) -> bool {
        self.device == other.device
    }
}

impl Eq for Endpoint {}

impl Clone for Endpoint {
    fn clone(&self) -> Endpoint {
        unsafe { (*self.device).AddRef(); }

        Endpoint {
            device: self.device,
            future_audio_client: self.future_audio_client.clone(),
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        unsafe { (*self.device).Release(); }

        if let Some(client) = self.future_audio_client.lock().unwrap().take() {
            unsafe { (*client.0).Release(); }
        }
    }
}

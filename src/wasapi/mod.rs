extern crate libc;
extern crate winapi;
extern crate ole32;

use std::io::Error as IoError;
use std::sync::{Arc, Mutex, MutexGuard};
use std::ptr;
use std::mem;

use Format;
use FormatsEnumerationError;
use SamplesRate;
use SampleFormat;

pub use std::option::IntoIter as OptionIntoIter;
pub use self::enumerate::{EndpointsIterator, get_default_endpoint};
pub use self::voice::{Voice, Buffer};

pub type SupportedFormatsIterator = OptionIntoIter<Format>;

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
            check_result((*client).GetMixFormat(&mut format_ptr)).unwrap();        // FIXME: don't unwrap

            let format = {
                assert!((*format_ptr).wFormatTag == winapi::WAVE_FORMAT_EXTENSIBLE);

                // FIXME: decode from the format
                Format {
                    channels: 2,
                    samples_rate: SamplesRate(44100),
                    data_type: SampleFormat::U16,
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

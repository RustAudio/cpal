use std::ffi::OsString;
use std::io::Error as IoError;
use std::mem;
use std::option::IntoIter as OptionIntoIter;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex, MutexGuard};

use ChannelCount;
use DefaultFormatError;
use Format;
use FormatsEnumerationError;
use SampleFormat;
use SampleRate;
use SupportedFormat;

use super::check_result;
use super::com;
use super::winapi::Interface;
use super::winapi::shared::ksmedia;
use super::winapi::shared::guiddef::{
    GUID,
};
use super::winapi::shared::mmreg::{
    WAVE_FORMAT_PCM,
    WAVE_FORMAT_EXTENSIBLE,
    WAVEFORMATEXTENSIBLE,
};
use super::winapi::um::audioclient::{
    IAudioClient,
    IID_IAudioClient,
    AUDCLNT_E_DEVICE_INVALIDATED,
};
use super::winapi::um::combaseapi::{
    CoCreateInstance,
    CoTaskMemFree,
    CLSCTX_ALL,
};
use super::winapi::um::mmdeviceapi::{
    eConsole,
    eRender,
    CLSID_MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
    IMMDevice,
    IMMDeviceCollection,
    IMMDeviceEnumerator,
};

pub type SupportedInputFormats = OptionIntoIter<SupportedFormat>;
pub type SupportedOutputFormats = OptionIntoIter<SupportedFormat>;

/// Wrapper because of that stupid decision to remove `Send` and `Sync` from raw pointers.
#[derive(Copy, Clone)]
struct IAudioClientWrapper(*mut IAudioClient);
unsafe impl Send for IAudioClientWrapper {
}
unsafe impl Sync for IAudioClientWrapper {
}

/// An opaque type that identifies an end point.
pub struct Device {
    device: *mut IMMDevice,
    /// We cache an uninitialized `IAudioClient` so that we can call functions from it without
    /// having to create/destroy audio clients all the time.
    future_audio_client: Arc<Mutex<Option<IAudioClientWrapper>>>, // TODO: add NonZero around the ptr
}

unsafe impl Send for Device {
}
unsafe impl Sync for Device {
}

impl Device {
    // TODO: this function returns a GUID of the device
    //       instead it should use the property store and return the friendly name
    pub fn name(&self) -> String {
        unsafe {
            let mut name_ptr = mem::uninitialized();
            // can only fail if wrong params or out of memory
            check_result((*self.device).GetId(&mut name_ptr)).unwrap();

            // finding the length of the name
            let mut len = 0;
            while *name_ptr.offset(len) != 0 {
                len += 1;
            }

            // building a slice containing the name
            let name_slice = slice::from_raw_parts(name_ptr, len as usize);

            // and turning it into a string
            let name_string: OsString = OsStringExt::from_wide(name_slice);
            CoTaskMemFree(name_ptr as *mut _);
            name_string.into_string().unwrap()
        }
    }

    #[inline]
    fn from_immdevice(device: *mut IMMDevice) -> Self {
        Device {
            device: device,
            future_audio_client: Arc::new(Mutex::new(None)),
        }
    }

    /// Ensures that `future_audio_client` contains a `Some` and returns a locked mutex to it.
    fn ensure_future_audio_client(&self)
                                  -> Result<MutexGuard<Option<IAudioClientWrapper>>, IoError> {
        let mut lock = self.future_audio_client.lock().unwrap();
        if lock.is_some() {
            return Ok(lock);
        }

        let audio_client: *mut IAudioClient = unsafe {
            let mut audio_client = mem::uninitialized();
            let hresult = (*self.device).Activate(&IID_IAudioClient,
                                                  CLSCTX_ALL,
                                                  ptr::null_mut(),
                                                  &mut audio_client);

            // can fail if the device has been disconnected since we enumerated it, or if
            // the device doesn't support playback for some reason
            check_result(hresult)?;
            assert!(!audio_client.is_null());
            audio_client as *mut _
        };

        *lock = Some(IAudioClientWrapper(audio_client));
        Ok(lock)
    }

    /// Returns an uninitialized `IAudioClient`.
    #[inline]
    pub(crate) fn build_audioclient(&self) -> Result<*mut IAudioClient, IoError> {
        let mut lock = self.ensure_future_audio_client()?;
        let client = lock.unwrap().0;
        *lock = None;
        Ok(client)
    }

    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, FormatsEnumerationError> {
        unimplemented!();
    }

    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, FormatsEnumerationError> {
        // We always create voices in shared mode, therefore all samples go through an audio
        // processor to mix them together.
        // However there is no way to query the list of all formats that are supported by the
        // audio processor, but one format is guaranteed to be supported, the one returned by
        // `GetMixFormat`.

        // initializing COM because we call `CoTaskMemFree`
        com::com_initialized();

        let lock = match self.ensure_future_audio_client() {
            Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) =>
                return Err(FormatsEnumerationError::DeviceNotAvailable),
            e => e.unwrap(),
        };
        let client = lock.unwrap().0;

        unsafe {
            let mut format_ptr = mem::uninitialized();
            match check_result((*client).GetMixFormat(&mut format_ptr)) {
                Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
                    return Err(FormatsEnumerationError::DeviceNotAvailable);
                },
                Err(e) => panic!("{:?}", e),
                Ok(()) => (),
            };

            let format = {
                let (channels, data_type) = match (*format_ptr).wFormatTag {
                    WAVE_FORMAT_PCM => {
                        (2, SampleFormat::I16)
                    },
                    WAVE_FORMAT_EXTENSIBLE => {
                        let format_ptr = format_ptr as *const WAVEFORMATEXTENSIBLE;
                        let channels = (*format_ptr).Format.nChannels as ChannelCount;
                        let format = {
                            fn cmp_guid(a: &GUID, b: &GUID) -> bool {
                                a.Data1 == b.Data1 && a.Data2 == b.Data2 && a.Data3 == b.Data3 &&
                                    a.Data4 == b.Data4
                            }
                            if cmp_guid(&(*format_ptr).SubFormat,
                                        &ksmedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT)
                            {
                                SampleFormat::F32
                            } else if cmp_guid(&(*format_ptr).SubFormat,
                                               &ksmedia::KSDATAFORMAT_SUBTYPE_PCM)
                            {
                                SampleFormat::I16
                            } else {
                                panic!("Unknown SubFormat GUID returned by GetMixFormat");
                                        // TODO: Re-add this to end of panic. Getting
                                        // `trait Debug is not satisfied` error.
                                       //(*format_ptr).SubFormat)
                            }
                        };

                        (channels, format)
                    },

                    f => panic!("Unknown data format returned by GetMixFormat: {:?}", f),
                };

                SupportedFormat {
                    channels: channels,
                    min_sample_rate: SampleRate((*format_ptr).nSamplesPerSec),
                    max_sample_rate: SampleRate((*format_ptr).nSamplesPerSec),
                    data_type: data_type,
                }
            };

            CoTaskMemFree(format_ptr as *mut _);

            Ok(Some(format).into_iter())
        }
    }

    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!();
    }

    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!();
    }
}

impl PartialEq for Device {
    #[inline]
    fn eq(&self, other: &Device) -> bool {
        self.device == other.device
    }
}

impl Eq for Device {
}

impl Clone for Device {
    #[inline]
    fn clone(&self) -> Device {
        unsafe {
            (*self.device).AddRef();
        }

        Device {
            device: self.device,
            future_audio_client: self.future_audio_client.clone(),
        }
    }
}

impl Drop for Device {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.device).Release();
        }

        if let Some(client) = self.future_audio_client.lock().unwrap().take() {
            unsafe {
                (*client.0).Release();
            }
        }
    }
}

lazy_static! {
    static ref ENUMERATOR: Enumerator = {
        // COM initialization is thread local, but we only need to have COM initialized in the
        // thread we create the objects in
        com::com_initialized();

        // building the devices enumerator object
        unsafe {
            let mut enumerator: *mut IMMDeviceEnumerator = mem::uninitialized();

            let hresult = CoCreateInstance(&CLSID_MMDeviceEnumerator,
                                                  ptr::null_mut(), CLSCTX_ALL,
                                                  &IMMDeviceEnumerator::uuidof(),
                                                  &mut enumerator
                                                           as *mut *mut IMMDeviceEnumerator
                                                           as *mut _);

            check_result(hresult).unwrap();
            Enumerator(enumerator)
        }
    };
}

/// RAII object around `IMMDeviceEnumerator`.
struct Enumerator(*mut IMMDeviceEnumerator);

unsafe impl Send for Enumerator {
}
unsafe impl Sync for Enumerator {
}

impl Drop for Enumerator {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.0).Release();
        }
    }
}

/// WASAPI implementation for `Devices`.
pub struct Devices {
    collection: *mut IMMDeviceCollection,
    total_count: u32,
    next_item: u32,
}

unsafe impl Send for Devices {
}
unsafe impl Sync for Devices {
}

impl Drop for Devices {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.collection).Release();
        }
    }
}

impl Default for Devices {
    fn default() -> Devices {
        unsafe {
            let mut collection: *mut IMMDeviceCollection = mem::uninitialized();
            // can fail because of wrong parameters (should never happen) or out of memory
            check_result((*ENUMERATOR.0).EnumAudioEndpoints(eRender,
                                                            DEVICE_STATE_ACTIVE,
                                                            &mut collection))
                .unwrap();

            let mut count = mem::uninitialized();
            // can fail if the parameter is null, which should never happen
            check_result((*collection).GetCount(&mut count)).unwrap();

            Devices {
                collection: collection,
                total_count: count,
                next_item: 0,
            }
        }
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let mut device = mem::uninitialized();
            // can fail if out of range, which we just checked above
            check_result((*self.collection).Item(self.next_item, &mut device)).unwrap();

            self.next_item += 1;
            Some(Device::from_immdevice(device))
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let num = self.total_count - self.next_item;
        let num = num as usize;
        (num, Some(num))
    }
}

pub fn default_input_device() -> Option<Device> {
    unimplemented!();
}

pub fn default_output_device() -> Option<Device> {
    unsafe {
        let mut device = mem::uninitialized();
        let hres = (*ENUMERATOR.0)
            .GetDefaultAudioEndpoint(eRender, eConsole, &mut device);
        if let Err(_err) = check_result(hres) {
            return None; // TODO: check specifically for `E_NOTFOUND`, and panic otherwise
        }
        Some(Device::from_immdevice(device))
    }
}

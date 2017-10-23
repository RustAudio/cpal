use std::ffi::OsString;
use std::io::Error as IoError;
use std::mem;
use std::option::IntoIter as OptionIntoIter;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex, MutexGuard};

use ChannelPosition;
use FormatsEnumerationError;
use SampleFormat;
use SamplesRate;
use SupportedFormat;

use super::check_result;
use super::com;
use super::ole32;
use super::winapi;

pub type SupportedFormatsIterator = OptionIntoIter<SupportedFormat>;

/// Wrapper because of that stupid decision to remove `Send` and `Sync` from raw pointers.
#[derive(Copy, Clone)]
struct IAudioClientWrapper(*mut winapi::IAudioClient);
unsafe impl Send for IAudioClientWrapper {
}
unsafe impl Sync for IAudioClientWrapper {
}

/// An opaque type that identifies an end point.
pub struct Endpoint {
    device: *mut winapi::IMMDevice,

    /// We cache an uninitialized `IAudioClient` so that we can call functions from it without
    /// having to create/destroy audio clients all the time.
    future_audio_client: Arc<Mutex<Option<IAudioClientWrapper>>>, // TODO: add NonZero around the ptr
}

unsafe impl Send for Endpoint {
}
unsafe impl Sync for Endpoint {
}

impl Endpoint {
    // TODO: this function returns a GUID of the endpoin
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
            ole32::CoTaskMemFree(name_ptr as *mut _);
            name_string.into_string().unwrap()
        }
    }

    #[inline]
    fn from_immdevice(device: *mut winapi::IMMDevice) -> Endpoint {
        Endpoint {
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

        let audio_client: *mut winapi::IAudioClient = unsafe {
            let mut audio_client = mem::uninitialized();
            let hresult = (*self.device).Activate(&winapi::IID_IAudioClient,
                                                  winapi::CLSCTX_ALL,
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
    pub(crate) fn build_audioclient(&self) -> Result<*mut winapi::IAudioClient, IoError> {
        let mut lock = self.ensure_future_audio_client()?;
        let client = lock.unwrap().0;
        *lock = None;
        Ok(client)
    }

    pub fn supported_formats(&self) -> Result<SupportedFormatsIterator, FormatsEnumerationError> {
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
                        (vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
                         SampleFormat::I16)
                    },
                    winapi::WAVE_FORMAT_EXTENSIBLE => {
                        let format_ptr = format_ptr as *const winapi::WAVEFORMATEXTENSIBLE;

                        let channels = {
                            let mut channels = Vec::new();

                            let mask = (*format_ptr).dwChannelMask;
                            if (mask & winapi::SPEAKER_FRONT_LEFT) != 0 {
                                channels.push(ChannelPosition::FrontLeft);
                            }
                            if (mask & winapi::SPEAKER_FRONT_RIGHT) != 0 {
                                channels.push(ChannelPosition::FrontRight);
                            }
                            if (mask & winapi::SPEAKER_FRONT_CENTER) != 0 {
                                channels.push(ChannelPosition::FrontCenter);
                            }
                            if (mask & winapi::SPEAKER_LOW_FREQUENCY) != 0 {
                                channels.push(ChannelPosition::LowFrequency);
                            }
                            if (mask & winapi::SPEAKER_BACK_LEFT) != 0 {
                                channels.push(ChannelPosition::BackLeft);
                            }
                            if (mask & winapi::SPEAKER_BACK_RIGHT) != 0 {
                                channels.push(ChannelPosition::BackRight);
                            }
                            if (mask & winapi::SPEAKER_FRONT_LEFT_OF_CENTER) != 0 {
                                channels.push(ChannelPosition::FrontLeftOfCenter);
                            }
                            if (mask & winapi::SPEAKER_FRONT_RIGHT_OF_CENTER) != 0 {
                                channels.push(ChannelPosition::FrontRightOfCenter);
                            }
                            if (mask & winapi::SPEAKER_BACK_CENTER) != 0 {
                                channels.push(ChannelPosition::BackCenter);
                            }
                            if (mask & winapi::SPEAKER_SIDE_LEFT) != 0 {
                                channels.push(ChannelPosition::SideLeft);
                            }
                            if (mask & winapi::SPEAKER_SIDE_RIGHT) != 0 {
                                channels.push(ChannelPosition::SideRight);
                            }
                            if (mask & winapi::SPEAKER_TOP_CENTER) != 0 {
                                channels.push(ChannelPosition::TopCenter);
                            }
                            if (mask & winapi::SPEAKER_TOP_FRONT_LEFT) != 0 {
                                channels.push(ChannelPosition::TopFrontLeft);
                            }
                            if (mask & winapi::SPEAKER_TOP_FRONT_CENTER) != 0 {
                                channels.push(ChannelPosition::TopFrontCenter);
                            }
                            if (mask & winapi::SPEAKER_TOP_FRONT_RIGHT) != 0 {
                                channels.push(ChannelPosition::TopFrontRight);
                            }
                            if (mask & winapi::SPEAKER_TOP_BACK_LEFT) != 0 {
                                channels.push(ChannelPosition::TopBackLeft);
                            }
                            if (mask & winapi::SPEAKER_TOP_BACK_CENTER) != 0 {
                                channels.push(ChannelPosition::TopBackCenter);
                            }
                            if (mask & winapi::SPEAKER_TOP_BACK_RIGHT) != 0 {
                                channels.push(ChannelPosition::TopBackRight);
                            }

                            assert_eq!((*format_ptr).Format.nChannels as usize, channels.len());
                            channels
                        };

                        let format = {
                            fn cmp_guid(a: &winapi::GUID, b: &winapi::GUID) -> bool {
                                a.Data1 == b.Data1 && a.Data2 == b.Data2 && a.Data3 == b.Data3 &&
                                    a.Data4 == b.Data4
                            }
                            if cmp_guid(&(*format_ptr).SubFormat,
                                        &winapi::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT)
                            {
                                SampleFormat::F32
                            } else if cmp_guid(&(*format_ptr).SubFormat,
                                               &winapi::KSDATAFORMAT_SUBTYPE_PCM)
                            {
                                SampleFormat::I16
                            } else {
                                panic!("Unknown SubFormat GUID returned by GetMixFormat: {:?}",
                                       (*format_ptr).SubFormat)
                            }
                        };

                        (channels, format)
                    },

                    f => panic!("Unknown data format returned by GetMixFormat: {:?}", f),
                };

                SupportedFormat {
                    channels: channels,
                    min_samples_rate: SamplesRate((*format_ptr).nSamplesPerSec),
                    max_samples_rate: SamplesRate((*format_ptr).nSamplesPerSec),
                    data_type: data_type,
                }
            };

            ole32::CoTaskMemFree(format_ptr as *mut _);

            Ok(Some(format).into_iter())
        }
    }
}

impl PartialEq for Endpoint {
    #[inline]
    fn eq(&self, other: &Endpoint) -> bool {
        self.device == other.device
    }
}

impl Eq for Endpoint {
}

impl Clone for Endpoint {
    #[inline]
    fn clone(&self) -> Endpoint {
        unsafe {
            (*self.device).AddRef();
        }

        Endpoint {
            device: self.device,
            future_audio_client: self.future_audio_client.clone(),
        }
    }
}

impl Drop for Endpoint {
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
            let mut enumerator: *mut winapi::IMMDeviceEnumerator = mem::uninitialized();

            let hresult = ole32::CoCreateInstance(&winapi::CLSID_MMDeviceEnumerator,
                                                  ptr::null_mut(), winapi::CLSCTX_ALL,
                                                  &winapi::IID_IMMDeviceEnumerator,
                                                  &mut enumerator
                                                           as *mut *mut winapi::IMMDeviceEnumerator
                                                           as *mut _);

            check_result(hresult).unwrap();
            Enumerator(enumerator)
        }
    };
}

/// RAII object around `winapi::IMMDeviceEnumerator`.
struct Enumerator(*mut winapi::IMMDeviceEnumerator);

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

/// WASAPI implementation for `EndpointsIterator`.
pub struct EndpointsIterator {
    collection: *mut winapi::IMMDeviceCollection,
    total_count: u32,
    next_item: u32,
}

unsafe impl Send for EndpointsIterator {
}
unsafe impl Sync for EndpointsIterator {
}

impl Drop for EndpointsIterator {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.collection).Release();
        }
    }
}

impl Default for EndpointsIterator {
    fn default() -> EndpointsIterator {
        unsafe {
            let mut collection: *mut winapi::IMMDeviceCollection = mem::uninitialized();
            // can fail because of wrong parameters (should never happen) or out of memory
            check_result((*ENUMERATOR.0).EnumAudioEndpoints(winapi::eRender,
                                                            winapi::DEVICE_STATE_ACTIVE,
                                                            &mut collection))
                .unwrap();

            let mut count = mem::uninitialized();
            // can fail if the parameter is null, which should never happen
            check_result((*collection).GetCount(&mut count)).unwrap();

            EndpointsIterator {
                collection: collection,
                total_count: count,
                next_item: 0,
            }
        }
    }
}

impl Iterator for EndpointsIterator {
    type Item = Endpoint;

    fn next(&mut self) -> Option<Endpoint> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let mut device = mem::uninitialized();
            // can fail if out of range, which we just checked above
            check_result((*self.collection).Item(self.next_item, &mut device)).unwrap();

            self.next_item += 1;
            Some(Endpoint::from_immdevice(device))
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let num = self.total_count - self.next_item;
        let num = num as usize;
        (num, Some(num))
    }
}

pub fn default_endpoint() -> Option<Endpoint> {
    unsafe {
        let mut device = mem::uninitialized();
        let hres = (*ENUMERATOR.0)
            .GetDefaultAudioEndpoint(winapi::eRender, winapi::eConsole, &mut device);

        if let Err(_err) = check_result(hres) {
            return None; // TODO: check specifically for `E_NOTFOUND`, and panic otherwise
        }

        Some(Endpoint::from_immdevice(device))
    }
}

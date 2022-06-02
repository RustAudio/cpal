use crate::{
    BackendSpecificError, BufferSize, Data, DefaultStreamConfigError, DeviceNameError,
    DevicesError, InputCallbackInfo, OutputCallbackInfo, SampleFormat, SampleRate, StreamConfig,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError, COMMON_SAMPLE_RATES,
};
use std;
use std::ffi::OsString;
use std::fmt;
use std::io::Error as IoError;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex, MutexGuard};

use super::check_result;
use super::check_result_backend_specific;
use super::com;
use std::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Devices::Properties;
use windows::Win32::Media::{Audio, KernelStreaming, Multimedia};
use windows::Win32::Foundation;
use windows::Win32::System::Com;
use windows::Win32::System::Ole;
use windows::Win32::System::Com::StructuredStorage;
use windows::Win32::System::Threading;
use windows::core::GUID;

// https://msdn.microsoft.com/en-us/library/cc230355.aspx
// use super::winapi::um::audioclient::{
//     self, IAudioClient, IID_IAudioClient, AUDCLNT_E_DEVICE_INVALIDATED,
// };
// use super::winapi::um::audiosessiontypes::{
//     AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
// };
// use super::winapi::um::combaseapi::{
//     CoCreateInstance, CoTaskMemFree, PropVariantClear, CLSCTX_ALL,
// };
// use super::winapi::um::coml2api;
// use super::winapi::um::mmdeviceapi::{
//     eAll, Audio::eCapture, eConsole, Audio::eRender, CLSID_MMDeviceEnumerator, EDataFlow, IMMDevice,
//     IMMDeviceCollection, IMMDeviceEnumerator, IMMEndpoint, DEVICE_STATE_ACTIVE,
// };
// use super::winapi::um::winnt::{LPWSTR, WCHAR};

use super::{
    stream::{AudioClientFlow, Stream, StreamInner},
//    winapi::um::synchapi,
};
use crate::{traits::DeviceTrait, BuildStreamError, StreamError};

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;

/// Wrapper because of that stupid decision to remove `Send` and `Sync` from raw pointers.
#[derive(Clone)]
struct IAudioClientWrapper(Audio::IAudioClient);
unsafe impl Send for IAudioClientWrapper {}
unsafe impl Sync for IAudioClientWrapper {}

/// An opaque type that identifies an end point.
pub struct Device {
    device: Audio::IMMDevice,
    /// We cache an uninitialized `IAudioClient` so that we can call functions from it without
    /// having to create/destroy audio clients all the time.
    future_audio_client: Arc<Mutex<Option<IAudioClientWrapper>>>, // TODO: add NonZero around the ptr
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Device::supported_input_configs(self)
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Device::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner = self.build_input_stream_raw_inner(config, sample_format)?;
        Ok(Stream::new_input(
            stream_inner,
            data_callback,
            error_callback,
        ))
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner = self.build_output_stream_raw_inner(config, sample_format)?;
        Ok(Stream::new_output(
            stream_inner,
            data_callback,
            error_callback,
        ))
    }
}

struct Endpoint {
    endpoint: Audio::IMMEndpoint,
}

enum WaveFormat {
    Ex(Audio::WAVEFORMATEX),
    Extensible(Audio::WAVEFORMATEXTENSIBLE),
}

// Use RAII to make sure CoTaskMemFree is called when we are responsible for freeing.
struct WaveFormatExPtr(*mut Audio::WAVEFORMATEX);

impl Drop for WaveFormatExPtr {
    fn drop(&mut self) {
        unsafe {
            Com::CoTaskMemFree(self.0 as *mut _);
        }
    }
}

impl WaveFormat {
    // Given a pointer to some format, returns a valid copy of the format.
    pub fn copy_from_waveformatex_ptr(ptr: *const Audio::WAVEFORMATEX) -> Option<Self> {
        unsafe {
            match (*ptr).wFormatTag as u32 {
                Audio::WAVE_FORMAT_PCM | Multimedia::WAVE_FORMAT_IEEE_FLOAT => {
                    Some(WaveFormat::Ex(*ptr))
                }
                KernelStreaming::WAVE_FORMAT_EXTENSIBLE => {
                    let extensible_ptr = ptr as *const Audio::WAVEFORMATEXTENSIBLE;
                    Some(WaveFormat::Extensible(*extensible_ptr))
                }
                _ => None,
            }
        }
    }

    // Get the pointer to the WAVEFORMATEX struct.
    pub fn as_ptr(&self) -> *const Audio::WAVEFORMATEX {
        self.deref() as *const _
    }
}

impl Deref for WaveFormat {
    type Target = Audio::WAVEFORMATEX;
    fn deref(&self) -> &Self::Target {
        match *self {
            WaveFormat::Ex(ref f) => f,
            WaveFormat::Extensible(ref f) => &f.Format,
        }
    }
}

impl DerefMut for WaveFormat {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match *self {
            WaveFormat::Ex(ref mut f) => f,
            WaveFormat::Extensible(ref mut f) => &mut f.Format,
        }
    }
}

unsafe fn immendpoint_from_immdevice(device: Audio::IMMDevice) -> Audio::IMMEndpoint {
    device.cast::<Audio::IMMEndpoint>().expect("could not query IMMDevice interface for IMMEndpoint")
}

unsafe fn data_flow_from_immendpoint(endpoint: Audio::IMMEndpoint) -> Audio::EDataFlow {
    endpoint.GetDataFlow()
        .expect("could not get endpoint data_flow")
}

// Given the audio client and format, returns whether or not the format is supported.
pub unsafe fn is_format_supported(
    client: Audio::IAudioClient,
    waveformatex_ptr: *const Audio::WAVEFORMATEX,
) -> Result<bool, SupportedStreamConfigsError> {
    /*
    // `IsFormatSupported` checks whether the format is supported and fills
    // a `WAVEFORMATEX`
    let mut dummy_fmt_ptr: *mut Audio::WAVEFORMATEX = mem::uninitialized();
    let hresult =
        audio_client
            .IsFormatSupported(share_mode, &format_attempt.Format, &mut dummy_fmt_ptr);
    // we free that `WAVEFORMATEX` immediately after because we don't need it
    if !dummy_fmt_ptr.is_null() {
        CoTaskMemFree(dummy_fmt_ptr as *mut _);
    }

    // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
    // has been found), but we also treat this as an error
    match (hresult, check_result(hresult)) {
        (_, Err(ref e))
            if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
            audio_client.Release();
            return Err(BuildStreamError::DeviceNotAvailable);
        },
        (_, Err(e)) => {
            audio_client.Release();
            panic!("{:?}", e);
        },
        (Foundation::S_FALSE, _) => {
            audio_client.Release();
            return Err(BuildStreamError::StreamConfigNotSupported);
        },
        (_, Ok(())) => (),
    };
    */

    // Check if the given format is supported.
    let is_supported = |waveformatex_ptr, mut closest_waveformatex_ptr| {
        let result = client.IsFormatSupported(
            Audio::AUDCLNT_SHAREMODE_SHARED,
            waveformatex_ptr,
            &mut closest_waveformatex_ptr,
        );
        // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
        // has been found, but not an exact match) so we also treat this as unsupported.
        match (result, check_result(result)) {
            (_, Err(ref e)) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                Err(SupportedStreamConfigsError::DeviceNotAvailable)
            }
            (_, Err(_)) => Ok(false),
            (Foundation::S_FALSE, _) => Ok(false),
            (_, Ok(())) => Ok(true),
        }
    };

    // First we want to retrieve a pointer to the `WAVEFORMATEX`.
    // Although `GetMixFormat` writes the format to a given `WAVEFORMATEX` pointer,
    // the pointer itself may actually point to a `WAVEFORMATEXTENSIBLE` structure.
    // We check the wFormatTag to determine this and get a pointer to the correct type.
    match (*waveformatex_ptr).wFormatTag {
        Audio::WAVE_FORMAT_PCM | Multimedia::WAVE_FORMAT_IEEE_FLOAT => {
            let mut closest_waveformatex = *waveformatex_ptr;
            let closest_waveformatex_ptr = &mut closest_waveformatex as *mut _;
            is_supported(waveformatex_ptr, closest_waveformatex_ptr)
        }
        KernelStreaming::WAVE_FORMAT_EXTENSIBLE => {
            let waveformatextensible_ptr = waveformatex_ptr as *const Audio::WAVEFORMATEXTENSIBLE;
            let mut closest_waveformatextensible = *waveformatextensible_ptr;
            let closest_waveformatextensible_ptr = &mut closest_waveformatextensible as *mut _;
            let closest_waveformatex_ptr =
                closest_waveformatextensible_ptr as *mut Audio::WAVEFORMATEX;
            is_supported(waveformatex_ptr, closest_waveformatex_ptr)
        }
        _ => Ok(false),
    }
}

// Get a cpal Format from a WAVEFORMATEX.
unsafe fn format_from_waveformatex_ptr(
    waveformatex_ptr: *const Audio::WAVEFORMATEX,
) -> Option<SupportedStreamConfig> {
    fn cmp_guid(a: &GUID, b: &GUID) -> bool {
        a.Data1 == b.Data1 && a.Data2 == b.Data2 && a.Data3 == b.Data3 && a.Data4 == b.Data4
    }
    let sample_format = match (
        (*waveformatex_ptr).wBitsPerSample,
        (*waveformatex_ptr).wFormatTag,
    ) {
        (16, Audio::WAVE_FORMAT_PCM) => SampleFormat::I16,
        (32, Multimedia::WAVE_FORMAT_IEEE_FLOAT) => SampleFormat::F32,
        (n_bits, KernelStreaming::WAVE_FORMAT_EXTENSIBLE) => {
            let waveformatextensible_ptr = waveformatex_ptr as *const Audio::WAVEFORMATEXTENSIBLE;
            let sub = (*waveformatextensible_ptr).SubFormat;
            if n_bits == 16 && cmp_guid(&sub, &KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM) {
                SampleFormat::I16
            } else if n_bits == 32 && cmp_guid(&sub, &Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) {
                SampleFormat::F32
            } else {
                return None;
            }
        }
        // Unknown data format returned by GetMixFormat.
        _ => return None,
    };

    let format = SupportedStreamConfig {
        channels: (*waveformatex_ptr).nChannels as _,
        sample_rate: SampleRate((*waveformatex_ptr).nSamplesPerSec),
        buffer_size: SupportedBufferSize::Unknown,
        sample_format,
    };
    Some(format)
}

unsafe impl Send for Device {}
unsafe impl Sync for Device {}

impl Device {
    pub fn name(&self) -> Result<String, DeviceNameError> {
        unsafe {
            // Open the device's property store.
            let mut property_store = ptr::null_mut();
            self.device.OpenPropertyStore(StructuredStorage::STGM_READ, &mut property_store);

            // Get the endpoint's friendly-name property.
            let mut property_value = mem::zeroed();
            if let Err(err) = check_result((*property_store).GetValue(
                &Properties::DEVPKEY_Device_FriendlyName as *const _ as *const _,
                &mut property_value,
            )) {
                let description = format!("failed to retrieve name from property store: {}", err);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }

            // Read the friendly-name from the union data field, expecting a *const u16.
            if property_value.vt != Ole::VT_LPWSTR as _ {
                let description = format!(
                    "property store produced invalid data: {:?}",
                    property_value.vt
                );
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            let ptr_utf16 = *(&property_value.data as *const _ as *const *const u16);

            // Find the length of the friendly name.
            let mut len = 0;
            while *ptr_utf16.offset(len) != 0 {
                len += 1;
            }

            // Create the utf16 slice and convert it into a string.
            let name_slice = slice::from_raw_parts(ptr_utf16, len as usize);
            let name_os_string: OsString = OsStringExt::from_wide(name_slice);
            let name_string = match name_os_string.into_string() {
                Ok(string) => string,
                Err(os_string) => os_string.to_string_lossy().into(),
            };

            // Clean up the property.
            StructuredStorage::PropVariantClear(&mut property_value);

            Ok(name_string)
        }
    }

    #[inline]
    fn from_immdevice(device: Audio::IMMDevice) -> Self {
        Device {
            device,
            future_audio_client: Arc::new(Mutex::new(None)),
        }
    }

    /// Ensures that `future_audio_client` contains a `Some` and returns a locked mutex to it.
    fn ensure_future_audio_client(
        &self,
    ) -> Result<MutexGuard<Option<IAudioClientWrapper>>, IoError> {
        let mut lock = self.future_audio_client.lock().unwrap();
        if lock.is_some() {
            return Ok(lock);
        }

        let audio_client: Audio::IAudioClient = unsafe {
            let mut audio_client = ptr::null_mut();
            let hresult = self.device.Activate(
                &Audio::IAudioClient::IID,
                Com::CLSCTX_ALL,
                ptr::null_mut(),
                &mut audio_client,
            );

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
    pub(crate) fn build_audioclient(&self) -> Result<Audio::IAudioClient, IoError> {
        let mut lock = self.ensure_future_audio_client()?;
        let client = lock.unwrap().0;
        *lock = None;
        Ok(client)
    }

    // There is no way to query the list of all formats that are supported by the
    // audio processor, so instead we just trial some commonly supported formats.
    //
    // Common formats are trialed by first getting the default format (returned via
    // `GetMixFormat`) and then mutating that format with common sample rates and
    // querying them via `IsFormatSupported`.
    //
    // When calling `IsFormatSupported` with the shared-mode audio engine, only the default
    // number of channels seems to be supported. Any, more or less returns an invalid
    // parameter error. Thus, we just assume that the default number of channels is the only
    // number supported.
    fn supported_formats(&self) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        // initializing COM because we call `CoTaskMemFree` to release the format.
        com::com_initialized();

        // Retrieve the `IAudioClient`.
        let lock = match self.ensure_future_audio_client() {
            Ok(lock) => lock,
            Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                return Err(SupportedStreamConfigsError::DeviceNotAvailable)
            }
            Err(e) => {
                let description = format!("{}", e);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
        };
        let client = lock.unwrap().0;

        unsafe {
            // Retrieve the pointer to the default WAVEFORMATEX.
            let mut default_waveformatex_ptr = WaveFormatExPtr(ptr::null_mut());
            match check_result((*client).GetMixFormat(&mut default_waveformatex_ptr.0)) {
                Ok(()) => (),
                Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                    return Err(SupportedStreamConfigsError::DeviceNotAvailable);
                }
                Err(e) => {
                    let description = format!("{}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            };

            // If the default format can't succeed we have no hope of finding other formats.
            assert_eq!(
                is_format_supported(client, default_waveformatex_ptr.0)?,
                true
            );

            // Copy the format to use as a test format (as to avoid mutating the original format).
            let mut test_format = {
                match WaveFormat::copy_from_waveformatex_ptr(default_waveformatex_ptr.0) {
                    Some(f) => f,
                    // If the format is neither EX nor EXTENSIBLE we don't know how to work with it.
                    None => return Ok(vec![].into_iter()),
                }
            };

            // Begin testing common sample rates.
            //
            // NOTE: We should really be testing for whole ranges here, but it is infeasible to
            // test every sample rate up to the overflow limit as the `IsFormatSupported` method is
            // quite slow.
            let mut supported_sample_rates: Vec<u32> = Vec::new();
            for &rate in COMMON_SAMPLE_RATES {
                let rate = rate.0;
                test_format.nSamplesPerSec = rate;
                test_format.nAvgBytesPerSec =
                    rate * u32::from((*default_waveformatex_ptr.0).nBlockAlign);
                if is_format_supported(client, test_format.as_ptr())? {
                    supported_sample_rates.push(rate);
                }
            }

            // If the common rates don't include the default one, add the default.
            let default_sr = (*default_waveformatex_ptr.0).nSamplesPerSec as _;
            if !supported_sample_rates.iter().any(|&r| r == default_sr) {
                supported_sample_rates.push(default_sr);
            }

            // Reset the sample rate on the test format now that we're done.
            test_format.nSamplesPerSec = (*default_waveformatex_ptr.0).nSamplesPerSec;
            test_format.nAvgBytesPerSec = (*default_waveformatex_ptr.0).nAvgBytesPerSec;

            // TODO: Test the different sample formats?

            // Create the supported formats.
            let format = match format_from_waveformatex_ptr(default_waveformatex_ptr.0) {
                Some(fmt) => fmt,
                None => {
                    let description =
                        "could not create a `cpal::SupportedStreamConfig` from a `WAVEFORMATEX`"
                            .to_string();
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            };
            let mut supported_formats = Vec::with_capacity(supported_sample_rates.len());
            for rate in supported_sample_rates {
                supported_formats.push(SupportedStreamConfigRange {
                    channels: format.channels.clone(),
                    min_sample_rate: SampleRate(rate as _),
                    max_sample_rate: SampleRate(rate as _),
                    buffer_size: format.buffer_size.clone(),
                    sample_format: format.sample_format.clone(),
                })
            }
            Ok(supported_formats.into_iter())
        }
    }

    pub fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        if self.data_flow() == Audio::eCapture {
            self.supported_formats()
        // If it's an output device, assume no input formats.
        } else {
            Ok(vec![].into_iter())
        }
    }

    pub fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        if self.data_flow() == Audio::eRender {
            self.supported_formats()
        // If it's an input device, assume no output formats.
        } else {
            Ok(vec![].into_iter())
        }
    }

    // We always create voices in shared mode, therefore all samples go through an audio
    // processor to mix them together.
    //
    // One format is guaranteed to be supported, the one returned by `GetMixFormat`.
    fn default_format(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        // initializing COM because we call `CoTaskMemFree`
        com::com_initialized();

        let lock = match self.ensure_future_audio_client() {
            Ok(lock) => lock,
            Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                return Err(DefaultStreamConfigError::DeviceNotAvailable)
            }
            Err(e) => {
                let description = format!("{}", e);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
        };
        let client = lock.unwrap().0;

        unsafe {
            let mut format_ptr = WaveFormatExPtr(ptr::null_mut());
            match check_result(client.GetMixFormat(&mut format_ptr.0)) {
                Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                    return Err(DefaultStreamConfigError::DeviceNotAvailable);
                }
                Err(e) => {
                    let description = format!("{}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
                Ok(()) => (),
            };

            format_from_waveformatex_ptr(format_ptr.0)
                .ok_or(DefaultStreamConfigError::StreamTypeNotSupported)
        }
    }

    pub(crate) fn data_flow(&self) -> Audio::EDataFlow {
        let endpoint = Endpoint::from(self.device as *const _);
        endpoint.data_flow()
    }

    pub fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        if self.data_flow() == Audio::eCapture {
            self.default_format()
        } else {
            Err(DefaultStreamConfigError::StreamTypeNotSupported)
        }
    }

    pub fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let data_flow = self.data_flow();
        if data_flow == Audio::eRender {
            self.default_format()
        } else {
            Err(DefaultStreamConfigError::StreamTypeNotSupported)
        }
    }

    pub(crate) fn build_input_stream_raw_inner(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
    ) -> Result<StreamInner, BuildStreamError> {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match self.build_audioclient() {
                Ok(client) => client,
                Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                    return Err(BuildStreamError::DeviceNotAvailable)
                }
                Err(e) => {
                    let description = format!("{}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            };

            match config.buffer_size {
                BufferSize::Fixed(_) => {
                    // TO DO: We need IAudioClient3 to get buffersize ranges first
                    // Otherwise the supported ranges are unknown. In the meantime
                    // the smallest buffersize is selected and used.
                    return Err(BuildStreamError::StreamConfigNotSupported);
                }
                BufferSize::Default => (),
            };

            let mut stream_flags = Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK;

            if self.data_flow() == Audio::eRender {
                stream_flags |= Audio::AUDCLNT_STREAMFLAGS_LOOPBACK;
            }

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = config_to_waveformatextensible(config, sample_format)
                    .ok_or(BuildStreamError::StreamConfigNotSupported)?;
                let share_mode = Audio::AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(BuildStreamError::StreamConfigNotSupported),
                    Err(_) => return Err(BuildStreamError::DeviceNotAvailable),
                    _ => (),
                }

                // Finally, initializing the audio client
                let hresult = audio_client.Initialize(
                    share_mode,
                    stream_flags,
                    0,
                    0,
                    &format_attempt.Format,
                    ptr::null(),
                );
                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        audio_client.Release();
                        return Err(BuildStreamError::DeviceNotAvailable);
                    }
                    Err(e) => {
                        audio_client.Release();
                        let description = format!("{}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    }
                    Ok(()) => (),
                };

                format_attempt.Format
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = {
                let mut max_frames_in_buffer = 0u32;
                let hresult = audio_client.GetBufferSize(&mut max_frames_in_buffer);

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        audio_client.Release();
                        return Err(BuildStreamError::DeviceNotAvailable);
                    }
                    Err(e) => {
                        audio_client.Release();
                        let description = format!("{}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    }
                    Ok(()) => (),
                };

                max_frames_in_buffer
            };

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event = {
                let event = Threading::CreateEventA(ptr::null_mut(), 0, 0, ptr::null());
                if event.is_null() {
                    audio_client.Release();
                    let description = "failed to create event".to_string();
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                if let Err(e) = check_result(audio_client.SetEventHandle(event)) {
                    audio_client.Release();
                    let description = format!("failed to call SetEventHandle: {}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                event
            };

            // Building a `IAudioCaptureClient` that will be used to read captured samples.
            let capture_client = {
                let mut capture_client: *mut Audio::IAudioCaptureClient = ptr::null_mut();
                let hresult = audio_client.GetService(
                    &Audio::IAudioCaptureClient::IID,
                    &mut capture_client as *mut *mut Audio::IAudioCaptureClient as *mut _,
                );

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        audio_client.Release();
                        return Err(BuildStreamError::DeviceNotAvailable);
                    }
                    Err(e) => {
                        audio_client.Release();
                        let description = format!("failed to build capture client: {}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    }
                    Ok(()) => (),
                };

                &mut *capture_client
            };

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            let client_flow = AudioClientFlow::Capture { capture_client };

            let audio_clock = get_audio_clock(audio_client).map_err(|err| {
                audio_client.Release();
                err
            })?;

            Ok(StreamInner {
                audio_client,
                audio_clock,
                client_flow,
                event,
                playing: false,
                max_frames_in_buffer,
                bytes_per_frame: waveformatex.nBlockAlign,
                config: config.clone(),
                sample_format,
            })
        }
    }

    pub(crate) fn build_output_stream_raw_inner(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
    ) -> Result<StreamInner, BuildStreamError> {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match self.build_audioclient() {
                Ok(client) => client,
                Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                    return Err(BuildStreamError::DeviceNotAvailable)
                }
                Err(e) => {
                    let description = format!("{}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            };

            match config.buffer_size {
                BufferSize::Fixed(_) => {
                    // TO DO: We need IAudioClient3 to get buffersize ranges first
                    // Otherwise the supported ranges are unknown. In the meantime
                    // the smallest buffersize is selected and used.
                    return Err(BuildStreamError::StreamConfigNotSupported);
                }
                BufferSize::Default => (),
            };

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = config_to_waveformatextensible(config, sample_format)
                    .ok_or(BuildStreamError::StreamConfigNotSupported)?;
                let share_mode = Audio::AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(BuildStreamError::StreamConfigNotSupported),
                    Err(_) => return Err(BuildStreamError::DeviceNotAvailable),
                    _ => (),
                }

                // Finally, initializing the audio client
                let hresult = audio_client.Initialize(
                    share_mode,
                    Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    0,
                    0,
                    &format_attempt.Format,
                    ptr::null(),
                );

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        audio_client.Release();
                        return Err(BuildStreamError::DeviceNotAvailable);
                    }
                    Err(e) => {
                        audio_client.Release();
                        let description = format!("{}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    }
                    Ok(()) => (),
                };

                format_attempt.Format
            };

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event = {
                let event = Threading::CreateEventA(ptr::null_mut(), 0, 0, ptr::null());
                if event.is_null() {
                    audio_client.Release();
                    let description = "failed to create event".to_string();
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                if let Err(e) = check_result(audio_client.SetEventHandle(event)) {
                    audio_client.Release();
                    let description = format!("failed to call SetEventHandle: {}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                };

                event
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = {
                let mut max_frames_in_buffer = 0u32;
                let hresult = audio_client.GetBufferSize(&mut max_frames_in_buffer);

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        audio_client.Release();
                        return Err(BuildStreamError::DeviceNotAvailable);
                    }
                    Err(e) => {
                        audio_client.Release();
                        let description = format!("failed to obtain buffer size: {}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    }
                    Ok(()) => (),
                };

                max_frames_in_buffer
            };

            // Building a `IAudioRenderClient` that will be used to fill the samples buffer.
            let render_client = {
                let mut render_client: *mut Audio::IAudioRenderClient = ptr::null_mut();
                let hresult = audio_client.GetService(
                    &Audio::IAudioRenderClient::IID,
                    &mut render_client as *mut *mut Audio::IAudioRenderClient as *mut _,
                );

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        audio_client.Release();
                        return Err(BuildStreamError::DeviceNotAvailable);
                    }
                    Err(e) => {
                        audio_client.Release();
                        let description = format!("failed to build render client: {}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    }
                    Ok(()) => (),
                };

                &mut *render_client
            };

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            let client_flow = AudioClientFlow::Render { render_client };

            let audio_clock = get_audio_clock(audio_client).map_err(|err| {
                audio_client.Release();
                err
            })?;

            Ok(StreamInner {
                audio_client,
                audio_clock,
                client_flow,
                event,
                playing: false,
                max_frames_in_buffer,
                bytes_per_frame: waveformatex.nBlockAlign,
                config: config.clone(),
                sample_format,
            })
        }
    }
}

impl PartialEq for Device {
    #[inline]
    fn eq(&self, other: &Device) -> bool {
        // Use case: In order to check whether the default device has changed
        // the client code might need to compare the previous default device with the current one.
        // The pointer comparison (`self.device == other.device`) don't work there,
        // because the pointers are different even when the default device stays the same.
        //
        // In this code section we're trying to use the GetId method for the device comparison, cf.
        // https://docs.microsoft.com/en-us/windows/desktop/api/mmdeviceapi/nf-mmdeviceapi-immdevice-getid
        unsafe {
            struct IdRAII(*mut u16);
            /// RAII for device IDs.
            impl Drop for IdRAII {
                fn drop(&mut self) {
                    unsafe { Com::CoTaskMemFree(self.0 as *mut c_void) }
                }
            }
            let mut id1: *mut u16 = ptr::null_mut();
            let rc1 = self.device.GetId(&mut id1);
            // GetId only fails with E_OUTOFMEMORY and if it does, we're probably dead already.
            // Plus it won't do to change the device comparison logic unexpectedly.
            if rc1 != Foundation::S_OK {
                panic!("cpal: GetId failure: {}", rc1)
            }
            let id1 = IdRAII(id1);
            let mut id2: *mut u16 = ptr::null_mut();
            let rc2 = (*other.device).GetId(&mut id2);
            if rc2 != Foundation::S_OK {
                panic!("cpal: GetId failure: {}", rc1)
            }
            let id2 = IdRAII(id2);
            // 16-bit null-terminated comparison.
            let mut offset = 0;
            loop {
                let w1: u16 = *id1.0.offset(offset);
                let w2: u16 = *id2.0.offset(offset);
                if w1 == 0 && w2 == 0 {
                    return true;
                }
                if w1 != w2 {
                    return false;
                }
                offset += 1;
            }
        }
    }
}

impl Eq for Device {}

impl Clone for Device {
    #[inline]
    fn clone(&self) -> Device {
        unsafe {
            self.device.AddRef();
        }

        Device {
            device: self.device,
            future_audio_client: self.future_audio_client.clone(),
        }
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("device", &self.device)
            .field("name", &self.name())
            .finish()
    }
}

impl Drop for Device {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            self.device.Release();
        }

        if let Some(client) = self.future_audio_client.lock().unwrap().take() {
            unsafe {
                (*client.0).Release();
            }
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        unsafe {
            (*self.endpoint).Release();
        }
    }
}

impl From<Audio::IMMDevice> for Endpoint {
    fn from(device: Audio::IMMDevice) -> Self {
        unsafe {
            let endpoint = immendpoint_from_immdevice(device);
            Endpoint { endpoint }
        }
    }
}

impl Endpoint {
    fn data_flow(&self) -> Audio::EDataFlow {
        unsafe { data_flow_from_immendpoint(self.endpoint) }
    }
}

lazy_static! {
    static ref ENUMERATOR: Enumerator = {
        // COM initialization is thread local, but we only need to have COM initialized in the
        // thread we create the objects in
        com::com_initialized();

        // building the devices enumerator object
        unsafe {
            let mut enumerator: Audio::IMMDeviceEnumerator = ptr::null_mut();

            let hresult = Com::CoCreateInstance::<Audio::IMMDeviceEnumerator>(
                &Audio::MMDeviceEnumerator,
                ptr::null_mut(),
                Com::CLSCTX_ALL,
            );

            check_result(hresult).unwrap();
            Enumerator(enumerator)
        }
    };
}

/// RAII objects around `IMMDeviceEnumerator`.
struct Enumerator(Audio::IMMDeviceEnumerator);

unsafe impl Send for Enumerator {}
unsafe impl Sync for Enumerator {}

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
    collection: Audio::IMMDeviceCollection,
    total_count: u32,
    next_item: u32,
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        unsafe {
            let mut collection: Audio::IMMDeviceCollection = ptr::null_mut();
            // can fail because of wrong parameters (should never happen) or out of memory
            check_result_backend_specific((*ENUMERATOR.0).EnumAudioEndpoints(
                Audio::eAll,
                Audio::DEVICE_STATE_ACTIVE,
                &mut collection,
            ))?;

            let count = 0u32;
            // can fail if the parameter is null, which should never happen
            check_result_backend_specific((*collection).GetCount(&count))?;

            Ok(Devices {
                collection,
                total_count: count,
                next_item: 0,
            })
        }
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

impl Drop for Devices {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.collection).Release();
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
            let mut device = ptr::null_mut();
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

fn default_device(data_flow: Audio::EDataFlow) -> Option<Device> {
    unsafe {
        let mut device = ptr::null_mut();
        let hres = (*ENUMERATOR.0).GetDefaultAudioEndpoint(data_flow, Audio::eConsole, &mut device);
        if let Err(_err) = check_result(hres) {
            return None; // TODO: check specifically for `E_NOTFOUND`, and panic otherwise
        }
        Some(Device::from_immdevice(device))
    }
}

pub fn default_input_device() -> Option<Device> {
    default_device(Audio::eCapture)
}

pub fn default_output_device() -> Option<Device> {
    default_device(Audio::eRender)
}

/// Get the audio clock used to produce `StreamInstant`s.
unsafe fn get_audio_clock(
    audio_client: *mut Audio::IAudioClient,
) -> Result<*mut Audio::IAudioClock, BuildStreamError> {
    let mut audio_clock: *mut Audio::IAudioClock = ptr::null_mut();
    let hresult = audio_client.GetService(
        &Audio::IAudioClock::IID,
        &mut audio_clock as *mut *mut Audio::IAudioClock as *mut _,
    );
    match check_result(hresult) {
        Err(ref e) if e.raw_os_error() == Some(Audio::AUDCLNT_E_DEVICE_INVALIDATED) => {
            return Err(BuildStreamError::DeviceNotAvailable);
        }
        Err(e) => {
            let description = format!("failed to build audio clock: {}", e);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }
        Ok(()) => (),
    };
    Ok(audio_clock)
}

// Turns a `Format` into a `WAVEFORMATEXTENSIBLE`.
//
// Returns `None` if the WAVEFORMATEXTENSIBLE does not support the given format.
fn config_to_waveformatextensible(
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Option<Audio::WAVEFORMATEXTENSIBLE> {
    let format_tag = match sample_format {
        SampleFormat::I16 => Audio::WAVE_FORMAT_PCM,
        SampleFormat::F32 => KernelStreaming::WAVE_FORMAT_EXTENSIBLE,
        SampleFormat::U16 => return None,
    };
    let channels = config.channels;
    let sample_rate = config.sample_rate.0;
    let sample_bytes = sample_format.sample_size() as u16;
    let avg_bytes_per_sec = u32::from(channels) * sample_rate * u32::from(sample_bytes);
    let block_align = channels * sample_bytes;
    let bits_per_sample = 8 * sample_bytes;
    let cb_size = match sample_format {
        SampleFormat::I16 => 0,
        SampleFormat::F32 => {
            let extensible_size = mem::size_of::<Audio::WAVEFORMATEXTENSIBLE>();
            let ex_size = mem::size_of::<Audio::WAVEFORMATEX>();
            (extensible_size - ex_size) as u16
        }
        SampleFormat::U16 => return None,
    };
    let waveformatex = Audio::WAVEFORMATEX {
        wFormatTag: format_tag,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: avg_bytes_per_sec,
        nBlockAlign: block_align,
        wBitsPerSample: bits_per_sample,
        cbSize: cb_size,
    };

    // CPAL does not care about speaker positions, so pass audio straight through.
    let channel_mask = KernelStreaming::KSAUDIO_SPEAKER_DIRECTOUT;

    let sub_format = match sample_format {
        SampleFormat::I16 => KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM,
        SampleFormat::F32 => Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        SampleFormat::U16 => return None,
    };
    let waveformatextensible = Audio::WAVEFORMATEXTENSIBLE {
        Format: waveformatex,
        Samples: bits_per_sample as u16,
        dwChannelMask: channel_mask,
        SubFormat: sub_format,
    };

    Some(waveformatextensible)
}

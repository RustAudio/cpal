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

use DefaultFormatError;
use Format;
use FormatsEnumerationError;
use SampleFormat;
use SampleRate;
use SupportedFormat;
use COMMON_SAMPLE_RATES;

use super::check_result;
use super::com;
use super::winapi::Interface;
use super::winapi::shared::devpkey;
use super::winapi::shared::ksmedia;
use super::winapi::shared::guiddef::{
    GUID,
};
use super::winapi::shared::winerror;
use super::winapi::shared::minwindef::{
    DWORD,
};
use super::winapi::shared::mmreg;
use super::winapi::shared::wtypes;
use super::winapi::um::coml2api;
use super::winapi::um::audioclient::{
    IAudioClient,
    IID_IAudioClient,
    AUDCLNT_E_DEVICE_INVALIDATED,
};
use super::winapi::um::audiosessiontypes::{
    AUDCLNT_SHAREMODE_SHARED,
};
use super::winapi::um::combaseapi::{
    CoCreateInstance,
    CoTaskMemFree,
    CLSCTX_ALL,
    PropVariantClear,
};
use super::winapi::um::mmdeviceapi::{
    eAll,
    eCapture,
    eConsole,
    eRender,
    CLSID_MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
    EDataFlow,
    IMMDevice,
    IMMDeviceCollection,
    IMMDeviceEnumerator,
    IMMEndpoint,
};

pub type SupportedInputFormats = std::vec::IntoIter<SupportedFormat>;
pub type SupportedOutputFormats = std::vec::IntoIter<SupportedFormat>;

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

struct Endpoint {
    endpoint: *mut IMMEndpoint,
}

enum WaveFormat {
    Ex(mmreg::WAVEFORMATEX),
    Extensible(mmreg::WAVEFORMATEXTENSIBLE),
}

// Use RAII to make sure CoTaskMemFree is called when we are responsible for freeing.
struct WaveFormatExPtr(*mut mmreg::WAVEFORMATEX);

impl Drop for WaveFormatExPtr {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(self.0 as *mut _);
        }
    }
}


impl WaveFormat {
    // Given a pointer to some format, returns a valid copy of the format.
    pub fn copy_from_waveformatex_ptr(ptr: *const mmreg::WAVEFORMATEX) -> Option<Self> {
        unsafe {
            match (*ptr).wFormatTag {
                mmreg::WAVE_FORMAT_PCM | mmreg::WAVE_FORMAT_IEEE_FLOAT => {
                    Some(WaveFormat::Ex(*ptr))
                },
                mmreg::WAVE_FORMAT_EXTENSIBLE => {
                    let extensible_ptr = ptr as *const mmreg::WAVEFORMATEXTENSIBLE;
                    Some(WaveFormat::Extensible(*extensible_ptr))
                },
                _ => None,
            }
        }
    }

    // Get the pointer to the WAVEFORMATEX struct.
    pub fn as_ptr(&self) -> *const mmreg::WAVEFORMATEX {
        self.deref() as *const _
    }
}

impl Deref for WaveFormat {
    type Target = mmreg::WAVEFORMATEX;
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


unsafe fn immendpoint_from_immdevice(device: *const IMMDevice) -> *mut IMMEndpoint {
    let mut endpoint: *mut IMMEndpoint = mem::uninitialized();
    check_result((*device).QueryInterface(&IMMEndpoint::uuidof(), &mut endpoint as *mut _ as *mut _))
        .expect("could not query IMMDevice interface for IMMEndpoint");
    endpoint
}

unsafe fn data_flow_from_immendpoint(endpoint: *const IMMEndpoint) -> EDataFlow {
    let mut data_flow = mem::uninitialized();
    check_result((*endpoint).GetDataFlow(&mut data_flow))
        .expect("could not get endpoint data_flow");
    data_flow
}

// Given the audio client and format, returns whether or not the format is supported.
pub unsafe fn is_format_supported(
    client: *const IAudioClient,
    waveformatex_ptr: *const mmreg::WAVEFORMATEX,
) -> Result<bool, FormatsEnumerationError>
{


    /*
    // `IsFormatSupported` checks whether the format is supported and fills
    // a `WAVEFORMATEX`
    let mut dummy_fmt_ptr: *mut mmreg::WAVEFORMATEX = mem::uninitialized();
    let hresult =
        (*audio_client)
            .IsFormatSupported(share_mode, &format_attempt.Format, &mut dummy_fmt_ptr);
    // we free that `WAVEFORMATEX` immediately after because we don't need it
    if !dummy_fmt_ptr.is_null() {
        CoTaskMemFree(dummy_fmt_ptr as *mut _);
    }

    // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
    // has been found) but we also treat this as an error
    match (hresult, check_result(hresult)) {
        (_, Err(ref e))
            if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
            (*audio_client).Release();
            return Err(CreationError::DeviceNotAvailable);
        },
        (_, Err(e)) => {
            (*audio_client).Release();
            panic!("{:?}", e);
        },
        (winerror::S_FALSE, _) => {
            (*audio_client).Release();
            return Err(CreationError::FormatNotSupported);
        },
        (_, Ok(())) => (),
    };
    */


    // Check if the given format is supported.
    let is_supported = |waveformatex_ptr, mut closest_waveformatex_ptr| {
        let result = (*client).IsFormatSupported(
            AUDCLNT_SHAREMODE_SHARED,
            waveformatex_ptr,
            &mut closest_waveformatex_ptr,
        );
        // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
        // has been found, but not an exact match) so we also treat this as unsupported.
        match (result, check_result(result)) {
            (_, Err(ref e)) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
                return Err(FormatsEnumerationError::DeviceNotAvailable);
            },
            (_, Err(_)) => {
                Ok(false)
            },
            (winerror::S_FALSE, _) => {
                Ok(false)
            },
            (_, Ok(())) => {
                Ok(true)
            },
        }
    };

    // First we want to retrieve a pointer to the `WAVEFORMATEX`.
    // Although `GetMixFormat` writes the format to a given `WAVEFORMATEX` pointer,
    // the pointer itself may actually point to a `WAVEFORMATEXTENSIBLE` structure.
    // We check the wFormatTag to determine this and get a pointer to the correct type.
    match (*waveformatex_ptr).wFormatTag {
        mmreg::WAVE_FORMAT_PCM | mmreg::WAVE_FORMAT_IEEE_FLOAT => {
            let mut closest_waveformatex = *waveformatex_ptr;
            let mut closest_waveformatex_ptr = &mut closest_waveformatex as *mut _;
            is_supported(waveformatex_ptr, closest_waveformatex_ptr)
        },
        mmreg::WAVE_FORMAT_EXTENSIBLE => {
            let waveformatextensible_ptr =
                waveformatex_ptr as *const mmreg::WAVEFORMATEXTENSIBLE;
            let mut closest_waveformatextensible = *waveformatextensible_ptr;
            let closest_waveformatextensible_ptr =
                &mut closest_waveformatextensible as *mut _;
            let mut closest_waveformatex_ptr =
                closest_waveformatextensible_ptr as *mut mmreg::WAVEFORMATEX;
            is_supported(waveformatex_ptr, closest_waveformatex_ptr)
        },
        _ => Ok(false),
    }
}


// Get a cpal Format from a WAVEFORMATEX.
unsafe fn format_from_waveformatex_ptr(
    waveformatex_ptr: *const mmreg::WAVEFORMATEX,
) -> Option<Format>
{
    fn cmp_guid(a: &GUID, b: &GUID) -> bool {
        a.Data1 == b.Data1
            && a.Data2 == b.Data2
            && a.Data3 == b.Data3
            && a.Data4 == b.Data4
    }
    let data_type = match ((*waveformatex_ptr).wBitsPerSample, (*waveformatex_ptr).wFormatTag) {
        (16, mmreg::WAVE_FORMAT_PCM) => SampleFormat::I16,
        (32, mmreg::WAVE_FORMAT_IEEE_FLOAT) => SampleFormat::F32,
        (n_bits, mmreg::WAVE_FORMAT_EXTENSIBLE) => {
            let waveformatextensible_ptr = waveformatex_ptr as *const mmreg::WAVEFORMATEXTENSIBLE;
            let sub = (*waveformatextensible_ptr).SubFormat;
            if n_bits == 16 && cmp_guid(&sub, &ksmedia::KSDATAFORMAT_SUBTYPE_PCM) {
                SampleFormat::I16
            } else if n_bits == 32 && cmp_guid(&sub, &ksmedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) {
                SampleFormat::F32
            } else {
                return None;
            }
        },
        // Unknown data format returned by GetMixFormat.
        _ => return None,
    };
    let format = Format {
        channels: (*waveformatex_ptr).nChannels as _,
        sample_rate: SampleRate((*waveformatex_ptr).nSamplesPerSec),
        data_type: data_type,
    };
    Some(format)
}

unsafe impl Send for Device {
}
unsafe impl Sync for Device {
}

impl Device {
    pub fn name(&self) -> String {
        unsafe {
            // Open the device's property store.
            let mut property_store = ptr::null_mut();
            (*self.device).OpenPropertyStore(coml2api::STGM_READ, &mut property_store);

            // Get the endpoint's friendly-name property.
            let mut property_value = mem::zeroed();
            check_result(
                (*property_store).GetValue(
                    &devpkey::DEVPKEY_Device_FriendlyName as *const _ as *const _,
                    &mut property_value
                )
            ).expect("failed to get friendly-name from property store");

            // Read the friendly-name from the union data field, expecting a *const u16.
            assert_eq!(property_value.vt, wtypes::VT_LPWSTR as _);
            let ptr_usize: usize = *(&property_value.data as *const _ as *const usize);
            let ptr_utf16 = ptr_usize as *const u16;

            // Find the length of the friendly name.
            let mut len = 0;
            while *ptr_utf16.offset(len) != 0 {
                len += 1;
            }

            // Create the utf16 slice and covert it into a string.
            let name_slice = slice::from_raw_parts(ptr_utf16, len as usize);
            let name_os_string: OsString = OsStringExt::from_wide(name_slice);
            let name_string = name_os_string.into_string().unwrap();

            // Clean up the property.
            PropVariantClear(&mut property_value);

            name_string
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

    // There is no way to query the list of all formats that are supported by the
    // audio processor, so instead we just trial some commonly supported formats.
    //
    // Common formats are trialed by first getting the default format (returned via
    // `GetMixFormat`) and then mutating that format with common sample rates and
    // querying them via `IsFormatSupported`.
    //
    // When calling `IsFormatSupported` with the shared-mode audio engine, only the default
    // number of channels seems to be supported. Any more or less returns an invalid
    // parameter error. Thus we just assume that the default number of channels is the only
    // number supported.
    fn supported_formats(&self) -> Result<SupportedInputFormats, FormatsEnumerationError> {
        // initializing COM because we call `CoTaskMemFree` to release the format.
        com::com_initialized();

        // Retrieve the `IAudioClient`.
        let lock = match self.ensure_future_audio_client() {
            Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) =>
                return Err(FormatsEnumerationError::DeviceNotAvailable),
            e => e.unwrap(),
        };
        let client = lock.unwrap().0;

        unsafe {
            // Retrieve the pointer to the default WAVEFORMATEX.
            let mut default_waveformatex_ptr = WaveFormatExPtr(mem::uninitialized());
            match check_result((*client).GetMixFormat(&mut default_waveformatex_ptr.0)) {
                Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
                    return Err(FormatsEnumerationError::DeviceNotAvailable);
                },
                Err(e) => panic!("{:?}", e),
                Ok(()) => (),
            };

            // If the default format can't succeed we have no hope of finding other formats.
            assert_eq!(try!(is_format_supported(client, default_waveformatex_ptr.0)), true);

            // Copy the format to use as a test format (as to avoid mutating the original format).
            let mut test_format = {
                match WaveFormat::copy_from_waveformatex_ptr(default_waveformatex_ptr.0) {
                    Some(f) => f,
                    // If the format is neither EX or EXTENSIBLE we don't know how to work with it.
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
                let rate = rate.0 as DWORD;
                test_format.nSamplesPerSec = rate;
                test_format.nAvgBytesPerSec =
                    rate * (*default_waveformatex_ptr.0).nBlockAlign as DWORD;
                if try!(is_format_supported(client, test_format.as_ptr())) {
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
            let mut format = format_from_waveformatex_ptr(default_waveformatex_ptr.0)
                .expect("could not create a cpal::Format from a WAVEFORMATEX");
            let mut supported_formats = Vec::with_capacity(supported_sample_rates.len());
            for rate in supported_sample_rates {
                format.sample_rate = SampleRate(rate as _);
                supported_formats.push(SupportedFormat::from(format.clone()));
            }

            Ok(supported_formats.into_iter())
        }
    }

    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, FormatsEnumerationError> {
        if self.data_flow() == eCapture {
            self.supported_formats()
        // If it's an output device, assume no input formats.
        } else {
            Ok(vec![].into_iter())
        }
    }

    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, FormatsEnumerationError> {
        if self.data_flow() == eRender {
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
    fn default_format(&self) -> Result<Format, DefaultFormatError> {
        // initializing COM because we call `CoTaskMemFree`
        com::com_initialized();

        let lock = match self.ensure_future_audio_client() {
            Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) =>
                return Err(DefaultFormatError::DeviceNotAvailable),
            e => e.unwrap(),
        };
        let client = lock.unwrap().0;

        unsafe {
            let mut format_ptr = WaveFormatExPtr(mem::uninitialized());
            match check_result((*client).GetMixFormat(&mut format_ptr.0)) {
                Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
                    return Err(DefaultFormatError::DeviceNotAvailable);
                },
                Err(e) => panic!("{:?}", e),
                Ok(()) => (),
            };

            format_from_waveformatex_ptr(format_ptr.0)
                .ok_or(DefaultFormatError::StreamTypeNotSupported)
        }
    }

    fn data_flow(&self) -> EDataFlow {
        let endpoint = Endpoint::from(self.device as *const _);
        endpoint.data_flow()
    }

    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        if self.data_flow() == eCapture {
            self.default_format()
        } else {
            Err(DefaultFormatError::StreamTypeNotSupported)
        }
    }

    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        let data_flow = self.data_flow();
        if data_flow == eRender {
            self.default_format()
        } else {
            Err(DefaultFormatError::StreamTypeNotSupported)
        }
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
            (*self.device).Release();
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

impl From<*const IMMDevice> for Endpoint {
    fn from(device: *const IMMDevice) -> Self {
        unsafe {
            let endpoint = immendpoint_from_immdevice(device);
            Endpoint { endpoint: endpoint }
        }
    }
}

impl Endpoint {
    fn data_flow(&self) -> EDataFlow {
        unsafe {
            data_flow_from_immendpoint(self.endpoint)
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

            let hresult = CoCreateInstance(
                &CLSID_MMDeviceEnumerator,
                ptr::null_mut(),
                CLSCTX_ALL,
                &IMMDeviceEnumerator::uuidof(),
                &mut enumerator as *mut *mut IMMDeviceEnumerator as *mut _,
            );

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
            check_result(
                (*ENUMERATOR.0).EnumAudioEndpoints(
                    eAll,
                    DEVICE_STATE_ACTIVE,
                    &mut collection,
                )
            ).unwrap();

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

fn default_device(data_flow: EDataFlow) -> Option<Device> {
    unsafe {
        let mut device = mem::uninitialized();
        let hres = (*ENUMERATOR.0)
            .GetDefaultAudioEndpoint(data_flow, eConsole, &mut device);
        if let Err(_err) = check_result(hres) {
            return None; // TODO: check specifically for `E_NOTFOUND`, and panic otherwise
        }
        Some(Device::from_immdevice(device))
    }
}

pub fn default_input_device() -> Option<Device> {
    default_device(eCapture)
}

pub fn default_output_device() -> Option<Device> {
    default_device(eRender)
}

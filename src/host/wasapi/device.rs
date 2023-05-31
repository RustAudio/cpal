use crate::FrameCount;
use crate::{
    BackendSpecificError, BufferSize, Data, DefaultStreamConfigError, DeviceNameError,
    DevicesError, InputCallbackInfo, OutputCallbackInfo, SampleFormat, SampleRate, StreamConfig,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError, COMMON_SAMPLE_RATES,
};
use once_cell::sync::Lazy;
use std::ffi::OsString;
use std::fmt;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::os::windows::ffi::OsStringExt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::com;
use super::com::bindings as wb;
use super::com::threading::create_event;
use super::{windows_err_to_cpal_err, windows_err_to_cpal_err_message};
use windows_core::{ComInterface, GUID};

use super::stream::{AudioClientFlow, Stream, StreamInner};
use crate::{traits::DeviceTrait, BuildStreamError, StreamError};

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;

/// Wrapper because of that stupid decision to remove `Send` and `Sync` from raw pointers.
#[derive(Clone)]
struct IAudioClientWrapper(wb::IAudioClient);
unsafe impl Send for IAudioClientWrapper {}
unsafe impl Sync for IAudioClientWrapper {}

/// An opaque type that identifies an end point.
#[derive(Clone)]
pub struct Device {
    device: wb::IMMDevice,
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
        _timeout: Option<Duration>,
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
        _timeout: Option<Duration>,
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
    endpoint: wb::IMMEndpoint,
}

enum WaveFormat {
    Ex(wb::WAVEFORMATEX),
    Extensible(wb::WAVEFORMATEXTENSIBLE),
}

// Use RAII to make sure CoTaskMemFree is called when we are responsible for freeing.
struct WaveFormatExPtr(*mut wb::WAVEFORMATEX);

impl Drop for WaveFormatExPtr {
    fn drop(&mut self) {
        unsafe {
            wb::CoTaskMemFree(self.0.cast());
        }
    }
}

// By default windows/windows-sys makes _all_ structs/unions Copy, the
// embedded bindings could have Copy implemented for specific structs, but it's
// really unnecessary for the limited number of cases in this file
#[inline]
unsafe fn memcpy<T>(src: *const T) -> T {
    let mut dst = std::mem::zeroed();
    std::ptr::copy_nonoverlapping(src, &mut dst, 1);
    dst
}

impl WaveFormat {
    // Given a pointer to some format, returns a valid copy of the format.
    pub fn copy_from_waveformatex_ptr(ptr: *const wb::WAVEFORMATEX) -> Option<Self> {
        unsafe {
            match (*ptr).wFormatTag as u32 {
                wb::WAVE_FORMAT_PCM | wb::WAVE_FORMAT_IEEE_FLOAT => {
                    Some(WaveFormat::Ex(memcpy(ptr)))
                }
                wb::WAVE_FORMAT_EXTENSIBLE => {
                    let extensible_ptr = ptr as *const wb::WAVEFORMATEXTENSIBLE;
                    Some(WaveFormat::Extensible(memcpy(extensible_ptr)))
                }
                _ => None,
            }
        }
    }

    // Get the pointer to the WAVEFORMATEX struct.
    pub fn as_ptr(&self) -> *const wb::WAVEFORMATEX {
        self.deref() as *const _
    }
}

impl Deref for WaveFormat {
    type Target = wb::WAVEFORMATEX;
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

#[inline]
unsafe fn immendpoint_from_immdevice(device: wb::IMMDevice) -> wb::IMMEndpoint {
    device
        .cast::<wb::IMMEndpoint>()
        .expect("could not query IMMDevice interface for IMMEndpoint")
}

#[inline]
unsafe fn data_flow_from_immendpoint(endpoint: &wb::IMMEndpoint) -> wb::EDataFlow {
    endpoint
        .GetDataFlow()
        .expect("could not get endpoint data_flow")
}

// Given the audio client and format, returns whether or not the format is supported.
pub unsafe fn is_format_supported(
    client: &wb::IAudioClient,
    waveformatex_ptr: *const wb::WAVEFORMATEX,
) -> Result<bool, SupportedStreamConfigsError> {
    // Check if the given format is supported.
    let is_supported = |waveformatex_ptr, closest_waveformatex_ptr| {
        let result = client.IsFormatSupported(
            wb::AUDCLNT_SHAREMODE_SHARED,
            waveformatex_ptr,
            std::ptr::NonNull::new(closest_waveformatex_ptr),
        );
        // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
        // has been found, but not an exact match) so we also treat this as unsupported.
        match result {
            wb::AUDCLNT_E_DEVICE_INVALIDATED => {
                Err(SupportedStreamConfigsError::DeviceNotAvailable)
            }
            r if r == wb::S_FALSE || r.is_err() => Ok(false),
            _ => Ok(true),
        }
    };

    // First we want to retrieve a pointer to the `WAVEFORMATEX`.
    // Although `GetMixFormat` writes the format to a given `WAVEFORMATEX` pointer,
    // the pointer itself may actually point to a `WAVEFORMATEXTENSIBLE` structure.
    // We check the wFormatTag to determine this and get a pointer to the correct type.
    match (*waveformatex_ptr).wFormatTag as u32 {
        wb::WAVE_FORMAT_PCM | wb::WAVE_FORMAT_IEEE_FLOAT => {
            let mut closest_waveformatex = memcpy(waveformatex_ptr);
            let mut closest_waveformatex_ptr = &mut closest_waveformatex as *mut _;
            is_supported(waveformatex_ptr, &mut closest_waveformatex_ptr as *mut _)
        }
        wb::WAVE_FORMAT_EXTENSIBLE => {
            let waveformatextensible_ptr = waveformatex_ptr as *const wb::WAVEFORMATEXTENSIBLE;
            let mut closest_waveformatextensible = memcpy(waveformatextensible_ptr);
            let closest_waveformatextensible_ptr = &mut closest_waveformatextensible as *mut _;
            let mut closest_waveformatex_ptr =
                closest_waveformatextensible_ptr as *mut wb::WAVEFORMATEX;
            is_supported(waveformatex_ptr, &mut closest_waveformatex_ptr as *mut _)
        }
        _ => Ok(false),
    }
}

// Get a cpal Format from a WAVEFORMATEX.
unsafe fn format_from_waveformatex_ptr(
    waveformatex_ptr: *const wb::WAVEFORMATEX,
    audio_client: &wb::IAudioClient,
) -> Option<SupportedStreamConfig> {
    fn cmp_guid(a: &GUID, b: &GUID) -> bool {
        (a.data1, a.data2, a.data3, a.data4) == (b.data1, b.data2, b.data3, b.data4)
    }
    let sample_format = match (
        (*waveformatex_ptr).wBitsPerSample,
        (*waveformatex_ptr).wFormatTag as u32,
    ) {
        (16, wb::WAVE_FORMAT_PCM) => SampleFormat::I16,
        (32, wb::WAVE_FORMAT_IEEE_FLOAT) => SampleFormat::F32,
        (n_bits, wb::WAVE_FORMAT_EXTENSIBLE) => {
            let waveformatextensible_ptr = waveformatex_ptr as *const wb::WAVEFORMATEXTENSIBLE;
            let sub = (*waveformatextensible_ptr).SubFormat;
            if n_bits == 16 && cmp_guid(&sub, &wb::KSDATAFORMAT_SUBTYPE_PCM) {
                SampleFormat::I16
            } else if n_bits == 32 && cmp_guid(&sub, &wb::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) {
                SampleFormat::F32
            } else {
                return None;
            }
        }
        // Unknown data format returned by GetMixFormat.
        _ => return None,
    };

    let sample_rate = SampleRate((*waveformatex_ptr).nSamplesPerSec);

    // GetBufferSizeLimits is only used for Hardware-Offloaded Audio
    // Processing, which was added in Windows 8, which places hardware
    // limits on the size of the audio buffer. If the sound system
    // *isn't* using offloaded audio, we're using a software audio
    // processing stack and have pretty much free rein to set buffer
    // size.
    //
    // In software audio stacks GetBufferSizeLimits returns
    // AUDCLNT_E_OFFLOAD_MODE_ONLY.
    //
    // https://docs.microsoft.com/en-us/windows-hardware/drivers/audio/hardware-offloaded-audio-processing
    let (mut min_buffer_duration, mut max_buffer_duration) = (0, 0);
    let buffer_size_is_limited = audio_client
        .cast::<wb::IAudioClient2>()
        .and_then(|audio_client| {
            audio_client.GetBufferSizeLimits(
                waveformatex_ptr,
                1,
                &mut min_buffer_duration,
                &mut max_buffer_duration,
            )
        })
        .is_ok();
    let buffer_size = if buffer_size_is_limited {
        SupportedBufferSize::Range {
            min: buffer_duration_to_frames(min_buffer_duration, sample_rate.0),
            max: buffer_duration_to_frames(max_buffer_duration, sample_rate.0),
        }
    } else {
        SupportedBufferSize::Range {
            min: 0,
            max: u32::max_value(),
        }
    };

    let format = SupportedStreamConfig {
        channels: (*waveformatex_ptr).nChannels as _,
        sample_rate,
        buffer_size,
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
            let property_store = self
                .device
                .OpenPropertyStore(wb::STGM_READ)
                .expect("could not open property store");

            // Get the endpoint's friendly-name property.
            let mut property_value = property_store
                .GetValue(((&wb::DEVPKEY_Device_FriendlyName) as *const wb::DEVPROPKEY).cast())
                .map_err(|err| {
                    let description =
                        format!("failed to retrieve name from property store: {}", err);
                    let err = BackendSpecificError { description };
                    DeviceNameError::from(err)
                })?;

            let prop_variant = &property_value.Anonymous.Anonymous;

            // Read the friendly-name from the union data field, expecting a *const u16.
            if prop_variant.vt != wb::VT_LPWSTR {
                let description = format!(
                    "property store produced invalid data: {:?}",
                    prop_variant.vt
                );
                let err = BackendSpecificError { description };
                return Err(err.into());
            }

            let name_slice = prop_variant.Anonymous.pwszVal.as_wide();
            let name_os_string: OsString = OsStringExt::from_wide(name_slice);
            let name_string = match name_os_string.into_string() {
                Ok(string) => string,
                Err(os_string) => os_string.to_string_lossy().into(),
            };

            // Clean up the property.
            let _ = wb::PropVariantClear(&mut property_value);

            Ok(name_string)
        }
    }

    #[inline]
    fn from_immdevice(device: wb::IMMDevice) -> Self {
        Device {
            device,
            future_audio_client: Arc::new(Mutex::new(None)),
        }
    }

    /// Ensures that `future_audio_client` contains a `Some` and returns a locked mutex to it.
    fn ensure_future_audio_client(
        &self,
    ) -> ::windows_core::Result<MutexGuard<Option<IAudioClientWrapper>>> {
        let mut lock = self.future_audio_client.lock().unwrap();
        if lock.is_some() {
            return Ok(lock);
        }

        let audio_client: wb::IAudioClient = unsafe {
            // can fail if the device has been disconnected since we enumerated it, or if
            // the device doesn't support playback for some reason
            self.device.Activate(wb::CLSCTX_ALL, None)?
        };

        *lock = Some(IAudioClientWrapper(audio_client));
        Ok(lock)
    }

    /// Returns an uninitialized `IAudioClient`.
    #[inline]
    pub(crate) fn build_audioclient(&self) -> ::windows_core::Result<wb::IAudioClient> {
        let mut lock = self.ensure_future_audio_client()?;
        Ok(lock.take().unwrap().0)
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
            Err(e) => {
                return Err(windows_err_to_cpal_err(e));
            }
        };
        let client = &lock.as_ref().unwrap().0;

        unsafe {
            // Retrieve the pointer to the default WAVEFORMATEX.
            let default_waveformatex_ptr = client
                .GetMixFormat()
                .map(WaveFormatExPtr)
                .map_err(windows_err_to_cpal_err::<SupportedStreamConfigsError>)?;

            // If the default format can't succeed we have no hope of finding other formats.
            assert!(is_format_supported(client, default_waveformatex_ptr.0)?);

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
            let format = match format_from_waveformatex_ptr(default_waveformatex_ptr.0, client) {
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
                    channels: format.channels,
                    min_sample_rate: SampleRate(rate as _),
                    max_sample_rate: SampleRate(rate as _),
                    buffer_size: format.buffer_size.clone(),
                    sample_format: format.sample_format,
                })
            }
            Ok(supported_formats.into_iter())
        }
    }

    pub fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        if self.data_flow() == wb::eCapture {
            self.supported_formats()
        // If it's an output device, assume no input formats.
        } else {
            Ok(vec![].into_iter())
        }
    }

    pub fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        if self.data_flow() == wb::eRender {
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
            Err(e) => {
                return Err(windows_err_to_cpal_err(e));
            }
        };
        let client = &lock.as_ref().unwrap().0;

        unsafe {
            let format_ptr = client
                .GetMixFormat()
                .map(WaveFormatExPtr)
                .map_err(windows_err_to_cpal_err::<DefaultStreamConfigError>)?;

            format_from_waveformatex_ptr(format_ptr.0, client)
                .ok_or(DefaultStreamConfigError::StreamTypeNotSupported)
        }
    }

    pub(crate) fn data_flow(&self) -> wb::EDataFlow {
        let endpoint = Endpoint::from(self.device.clone());
        endpoint.data_flow()
    }

    pub fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        if self.data_flow() == wb::eCapture {
            self.default_format()
        } else {
            Err(DefaultStreamConfigError::StreamTypeNotSupported)
        }
    }

    pub fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let data_flow = self.data_flow();
        if data_flow == wb::eRender {
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
                Err(e) => {
                    return Err(windows_err_to_cpal_err(e));
                }
            };

            let buffer_duration =
                buffer_size_to_duration(&config.buffer_size, config.sample_rate.0);

            let mut stream_flags = wb::AUDCLNT_STREAMFLAGS_EVENTCALLBACK;

            if self.data_flow() == wb::eRender {
                stream_flags |= wb::AUDCLNT_STREAMFLAGS_LOOPBACK;
            }

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = config_to_waveformatextensible(config, sample_format)
                    .ok_or(BuildStreamError::StreamConfigNotSupported)?;
                let share_mode = wb::AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(&audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(BuildStreamError::StreamConfigNotSupported),
                    Err(_) => return Err(BuildStreamError::DeviceNotAvailable),
                    _ => (),
                }

                // Finally, initializing the audio client
                if let Err(err) = audio_client.Initialize(
                    share_mode,
                    stream_flags,
                    buffer_duration,
                    0,
                    &format_attempt.Format,
                    None,
                ) {
                    return Err(windows_err_to_cpal_err(err));
                }

                format_attempt.Format
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = audio_client
                .GetBufferSize()
                .map_err(windows_err_to_cpal_err::<BuildStreamError>)?;

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event = {
                let event = create_event().map_err(|e| {
                    let description = format!("failed to create event: {}", e);
                    let err = BackendSpecificError { description };
                    BuildStreamError::from(err)
                })?;

                if let Err(e) = audio_client.SetEventHandle(event) {
                    let description = format!("failed to call SetEventHandle: {}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                event
            };

            // Building a `IAudioCaptureClient` that will be used to read captured samples.
            let capture_client = audio_client
                .GetService::<wb::IAudioCaptureClient>()
                .map_err(|e| {
                    windows_err_to_cpal_err_message::<BuildStreamError>(
                        e,
                        "failed to build capture client: ",
                    )
                })?;

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            let client_flow = AudioClientFlow::Capture { capture_client };

            let audio_clock = get_audio_clock(&audio_client)?;

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
            let audio_client = self
                .build_audioclient()
                .map_err(windows_err_to_cpal_err::<BuildStreamError>)?;

            let buffer_duration =
                buffer_size_to_duration(&config.buffer_size, config.sample_rate.0);

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = config_to_waveformatextensible(config, sample_format)
                    .ok_or(BuildStreamError::StreamConfigNotSupported)?;
                let share_mode = wb::AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(&audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(BuildStreamError::StreamConfigNotSupported),
                    Err(_) => return Err(BuildStreamError::DeviceNotAvailable),
                    _ => (),
                }

                // Finally, initializing the audio client
                audio_client
                    .Initialize(
                        share_mode,
                        wb::AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                        buffer_duration,
                        0,
                        &format_attempt.Format,
                        None,
                    )
                    .map_err(windows_err_to_cpal_err::<BuildStreamError>)?;

                format_attempt.Format
            };

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event = {
                let event = create_event().map_err(|e| {
                    let description = format!("failed to create event: {}", e);
                    let err = BackendSpecificError { description };
                    BuildStreamError::from(err)
                })?;

                if let Err(e) = audio_client.SetEventHandle(event) {
                    let description = format!("failed to call SetEventHandle: {}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                event
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = audio_client.GetBufferSize().map_err(|e| {
                windows_err_to_cpal_err_message::<BuildStreamError>(
                    e,
                    "failed to obtain buffer size: ",
                )
            })?;

            // Building a `IAudioRenderClient` that will be used to fill the samples buffer.
            let render_client = audio_client
                .GetService::<wb::IAudioRenderClient>()
                .map_err(|e| {
                    windows_err_to_cpal_err_message::<BuildStreamError>(
                        e,
                        "failed to build render client: ",
                    )
                })?;

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            let client_flow = AudioClientFlow::Render { render_client };

            let audio_clock = get_audio_clock(&audio_client)?;

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
            struct IdRAII(::windows_core::PWSTR);
            /// RAII for device IDs.
            impl Drop for IdRAII {
                fn drop(&mut self) {
                    unsafe { wb::CoTaskMemFree(self.0 .0.cast()) }
                }
            }
            // GetId only fails with E_OUTOFMEMORY and if it does, we're probably dead already.
            // Plus it won't do to change the device comparison logic unexpectedly.
            let id1 = IdRAII(self.device.GetId().expect("cpal: GetId failure"));
            let id2 = IdRAII(other.device.GetId().expect("cpal: GetId failure"));

            id1.0.as_wide() == id2.0.as_wide()
        }
    }
}

impl Eq for Device {}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("name", &self.name())
            .finish()
    }
}

impl From<wb::IMMDevice> for Endpoint {
    fn from(device: wb::IMMDevice) -> Self {
        unsafe {
            let endpoint = immendpoint_from_immdevice(device);
            Endpoint { endpoint }
        }
    }
}

impl Endpoint {
    fn data_flow(&self) -> wb::EDataFlow {
        unsafe { data_flow_from_immendpoint(&self.endpoint) }
    }
}

static ENUMERATOR: Lazy<Enumerator> = Lazy::new(|| {
    // COM initialization is thread local, but we only need to have COM initialized in the
    // thread we create the objects in
    com::com_initialized();

    // build the devices enumerator object
    // https://learn.microsoft.com/en-us/windows/win32/coreaudio/mmdevice-api
    unsafe {
        let mut iptr = std::mem::MaybeUninit::<wb::IMMDeviceEnumerator>::uninit();
        let res = wb::CoCreateInstance(
            &wb::MMDeviceEnumerator,
            std::ptr::null_mut(),
            wb::CLSCTX_ALL,
            &wb::IMMDeviceEnumerator::IID,
            iptr.as_mut_ptr().cast(),
        );

        if res.is_ok() {
            Enumerator(iptr.assume_init())
        } else {
            panic!(
                "failed to create device enumerator: {}",
                ::windows_core::Error::from(res)
            );
        }
    }
});

/// Send/Sync wrapper around `IMMDeviceEnumerator`.
struct Enumerator(wb::IMMDeviceEnumerator);

unsafe impl Send for Enumerator {}
unsafe impl Sync for Enumerator {}

/// WASAPI implementation for `Devices`.
pub struct Devices {
    collection: wb::IMMDeviceCollection,
    total_count: u32,
    next_item: u32,
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        unsafe {
            // can fail because of wrong parameters (should never happen) or out of memory
            let collection = ENUMERATOR
                .0
                .EnumAudioEndpoints(wb::eAll, wb::DEVICE_STATE_ACTIVE)
                .map_err(BackendSpecificError::from)?;

            let count = collection.GetCount().map_err(BackendSpecificError::from)?;

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

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let device = self.collection.Item(self.next_item).unwrap();
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

fn default_device(data_flow: wb::EDataFlow) -> Option<Device> {
    unsafe {
        let device = ENUMERATOR
            .0
            .GetDefaultAudioEndpoint(data_flow, wb::eConsole)
            .ok()?;
        // TODO: check specifically for `E_NOTFOUND`, and panic otherwise
        Some(Device::from_immdevice(device))
    }
}

pub fn default_input_device() -> Option<Device> {
    default_device(wb::eCapture)
}

pub fn default_output_device() -> Option<Device> {
    default_device(wb::eRender)
}

/// Get the audio clock used to produce `StreamInstant`s.
unsafe fn get_audio_clock(
    audio_client: &wb::IAudioClient,
) -> Result<wb::IAudioClock, BuildStreamError> {
    audio_client.GetService::<wb::IAudioClock>().map_err(|e| {
        windows_err_to_cpal_err_message::<BuildStreamError>(e, "failed to build audio clock: ")
    })
}

// Turns a `Format` into a `WAVEFORMATEXTENSIBLE`.
//
// Returns `None` if the WAVEFORMATEXTENSIBLE does not support the given format.
fn config_to_waveformatextensible(
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Option<wb::WAVEFORMATEXTENSIBLE> {
    let format_tag = match sample_format {
        SampleFormat::I16 => wb::WAVE_FORMAT_PCM,
        SampleFormat::F32 => wb::WAVE_FORMAT_EXTENSIBLE,
        _ => return None,
    } as u16;
    let channels = config.channels;
    let sample_rate = config.sample_rate.0;
    let sample_bytes = sample_format.sample_size() as u16;
    let avg_bytes_per_sec = u32::from(channels) * sample_rate * u32::from(sample_bytes);
    let block_align = channels * sample_bytes;
    let bits_per_sample = 8 * sample_bytes;
    let cb_size = match sample_format {
        SampleFormat::I16 => 0,
        SampleFormat::F32 => {
            let extensible_size = mem::size_of::<wb::WAVEFORMATEXTENSIBLE>();
            let ex_size = mem::size_of::<wb::WAVEFORMATEX>();
            (extensible_size - ex_size) as u16
        }
        _ => return None,
    };
    let waveformatex = wb::WAVEFORMATEX {
        wFormatTag: format_tag,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: avg_bytes_per_sec,
        nBlockAlign: block_align,
        wBitsPerSample: bits_per_sample,
        cbSize: cb_size,
    };

    // CPAL does not care about speaker positions, so pass audio ight through.
    let channel_mask = wb::KSAUDIO_SPEAKER_DIRECTOUT;

    let sub_format = match sample_format {
        SampleFormat::I16 => wb::KSDATAFORMAT_SUBTYPE_PCM,
        SampleFormat::F32 => wb::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        _ => return None,
    };
    let waveformatextensible = wb::WAVEFORMATEXTENSIBLE {
        Format: waveformatex,
        Samples: wb::WAVEFORMATEXTENSIBLE_0 {
            wSamplesPerBlock: bits_per_sample,
        },
        dwChannelMask: channel_mask,
        SubFormat: sub_format,
    };

    Some(waveformatextensible)
}

fn buffer_size_to_duration(buffer_size: &BufferSize, sample_rate: u32) -> i64 {
    match buffer_size {
        BufferSize::Fixed(frames) => *frames as i64 * (1_000_000_000 / 100) / sample_rate as i64,
        BufferSize::Default => 0,
    }
}

fn buffer_duration_to_frames(buffer_duration: i64, sample_rate: u32) -> FrameCount {
    (buffer_duration * sample_rate as i64 * 100 / 1_000_000_000) as FrameCount
}

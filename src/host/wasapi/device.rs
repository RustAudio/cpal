use crate::{
    error::ResultExt, host::com::ComString, BufferSize, Data, DeviceDescription,
    DeviceDescriptionBuilder, DeviceDirection, DeviceId, DeviceType, Error, ErrorKind, FrameCount,
    InputCallbackInfo, InterfaceType, OutputCallbackInfo, SampleFormat, SampleRate, StreamConfig,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange, COMMON_SAMPLE_RATES,
};

impl From<Audio::EDataFlow> for DeviceDirection {
    fn from(data_flow: Audio::EDataFlow) -> Self {
        if data_flow == Audio::eCapture {
            DeviceDirection::Input
        } else if data_flow == Audio::eRender {
            DeviceDirection::Output
        } else {
            DeviceDirection::Unknown
        }
    }
}
use std::{
    ffi::OsString,
    fmt,
    hash::Hash,
    mem,
    os::windows::ffi::OsStringExt,
    ptr, slice,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use windows::{
    core::{Interface, GUID},
    Win32::{
        Devices::Properties,
        Foundation::{ERROR_TIMEOUT, PROPERTYKEY},
        Media::{Audio, Audio::IAudioRenderClient, KernelStreaming, Multimedia},
        System::{
            Com,
            Com::{StructuredStorage, STGM_READ},
            Threading,
            Variant::{VT_LPWSTR, VT_UI4},
        },
        UI::Shell::PropertiesSystem::IPropertyStore,
    },
};

use super::stream::{AudioClientFlow, DefaultDeviceMonitor, Stream, StreamInner};
pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};
use crate::{host::com, traits::DeviceTrait};

// PKEY_AudioEndpoint properties not yet in windows-rs

/// PKEY_AudioEndpoint_FormFactor (PID 0) - VT_UI4 containing EndpointFormFactor enum
const PKEY_AUDIOENDPOINT_FORMFACTOR: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e),
    pid: 0,
};

/// PKEY_AudioEndpoint_JackSubType (PID 8) - VT_LPWSTR containing KS node type GUID
const PKEY_AUDIOENDPOINT_JACKSUBTYPE: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e),
    pid: 8,
};

const DEFAULT_FLAGS: u32 = Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK
    | Audio::AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY
    | Audio::AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM;

/// Wrapper because of that stupid decision to remove `Send` and `Sync` from raw pointers.
#[derive(Clone)]
struct IAudioClientWrapper(Audio::IAudioClient);
unsafe impl Send for IAudioClientWrapper {}
unsafe impl Sync for IAudioClientWrapper {}

/// Distinguishes how a `Device` was obtained so streams know which activation path to use.
#[derive(Clone, Debug)]
enum DeviceHandle {
    DefaultOutput,
    DefaultInput,
    Specific(Audio::IMMDevice),
}

/// An opaque type that identifies an end point.
#[derive(Clone)]
pub struct Device {
    device: DeviceHandle,
    /// We cache an uninitialized `IAudioClient` so that we can call functions from it without
    /// having to create/destroy audio clients all the time.
    future_audio_client: Arc<Mutex<Option<IAudioClientWrapper>>>, // TODO: add NonZero around the ptr
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Device::description(self)
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Self::id(self)
    }

    fn supports_input(&self) -> bool {
        self.data_flow() == Audio::eCapture
    }

    fn supports_output(&self) -> bool {
        self.data_flow() == Audio::eRender
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Self::supported_input_configs(self)
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        Self::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Self::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        Self::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let stream_inner = self.build_input_stream_raw_inner(config, sample_format, timeout)?;
        let monitor = self.default_device_monitor()?;
        Ok(Stream::new_input(
            stream_inner,
            data_callback,
            error_callback,
            monitor,
        ))
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let stream_inner = self.build_output_stream_raw_inner(config, sample_format, timeout)?;
        let monitor = self.default_device_monitor()?;
        Ok(Stream::new_output(
            stream_inner,
            data_callback,
            error_callback,
            monitor,
        ))
    }
}

struct Endpoint {
    endpoint: Audio::IMMEndpoint,
}

// Use RAII to make sure CoTaskMemFree is called when we are responsible for freeing.
struct WaveFormatExPtr(*mut Audio::WAVEFORMATEX);

impl Drop for WaveFormatExPtr {
    fn drop(&mut self) {
        unsafe {
            Com::CoTaskMemFree(Some(self.0 as *mut _));
        }
    }
}

unsafe fn immendpoint_from_immdevice(device: Audio::IMMDevice) -> Audio::IMMEndpoint {
    device
        .cast::<Audio::IMMEndpoint>()
        .expect("could not query IMMDevice interface for IMMEndpoint")
}

unsafe fn data_flow_from_immendpoint(endpoint: &Audio::IMMEndpoint) -> Audio::EDataFlow {
    endpoint
        .GetDataFlow()
        .expect("could not get endpoint data_flow")
}

// Given the audio client and format, returns whether or not the format is supported.
pub unsafe fn is_format_supported(
    _client: &Audio::IAudioClient,
    _waveformatex_ptr: *const Audio::WAVEFORMATEX,
) -> Result<bool, Error> {
    // Checking formats is not needed for shared mode with auto-conversion, therefore this check has been removed until someone implements WASAPI exclusive mode support
    // I used an NAudio issue as reference: https://github.com/naudio/NAudio/issues/819

    Ok(true)
}

// Get a cpal Format from a WAVEFORMATEX.
unsafe fn format_from_waveformatex_ptr(
    waveformatex_ptr: *const Audio::WAVEFORMATEX,
    audio_client: &Audio::IAudioClient,
) -> Option<SupportedStreamConfig> {
    fn cmp_guid(a: &GUID, b: &GUID) -> bool {
        (a.data1, a.data2, a.data3, a.data4) == (b.data1, b.data2, b.data3, b.data4)
    }
    let sample_format = match (
        (*waveformatex_ptr).wBitsPerSample,
        (*waveformatex_ptr).wFormatTag as u32,
    ) {
        (8, Audio::WAVE_FORMAT_PCM) => SampleFormat::U8,
        (16, Audio::WAVE_FORMAT_PCM) => SampleFormat::I16,
        (32, Multimedia::WAVE_FORMAT_IEEE_FLOAT) => SampleFormat::F32,
        (n_bits, KernelStreaming::WAVE_FORMAT_EXTENSIBLE) => {
            let waveformatextensible_ptr = waveformatex_ptr as *const Audio::WAVEFORMATEXTENSIBLE;
            let sub = (*waveformatextensible_ptr).SubFormat;

            if cmp_guid(&sub, &KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM) {
                match n_bits {
                    8 => SampleFormat::U8,
                    16 => SampleFormat::I16,
                    24 => SampleFormat::I24,
                    32 => SampleFormat::I32,
                    64 => SampleFormat::I64,
                    _ => return None,
                }
            } else if n_bits == 32 && cmp_guid(&sub, &Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) {
                SampleFormat::F32
            } else {
                return None;
            }
        }
        // Unknown data format returned by GetMixFormat.
        _ => return None,
    };

    let sample_rate = (*waveformatex_ptr).nSamplesPerSec;

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
        .cast::<Audio::IAudioClient2>()
        .and_then(|audio_client| {
            audio_client.GetBufferSizeLimits(
                waveformatex_ptr,
                true,
                &mut min_buffer_duration,
                &mut max_buffer_duration,
            )
        })
        .is_ok();
    let buffer_size = if buffer_size_is_limited {
        SupportedBufferSize::Range {
            min: buffer_duration_to_frames(min_buffer_duration, sample_rate),
            max: buffer_duration_to_frames(max_buffer_duration, sample_rate),
        }
    } else {
        SupportedBufferSize::Range {
            min: 0,
            max: u32::MAX,
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

/// Maps PKEY_AudioEndpoint_JackSubType GUID to InterfaceType.
///
/// The JackSubType property contains a KS node type GUID string from Ksmedia.h
/// that specifies the physical connector type.
fn jacksubtype_to_interface_type(guid_str: &str) -> Option<InterfaceType> {
    let guid_upper = guid_str.to_uppercase();
    let typ = match guid_upper.as_str() {
        "{D9E55EA0-0C89-4692-84FF-EB3C4B0D172F}" => InterfaceType::Hdmi,
        "{E47E4031-3EA6-418D-8F9B-B73843CCB2AD}" => InterfaceType::DisplayPort,
        "{DFF21CE1-F70F-11D0-B917-00A0C9223196}" => InterfaceType::Spdif,
        _ => return None,
    };

    Some(typ)
}

/// Maps WASAPI FormFactor values to DeviceType and optionally InterfaceType.
fn form_factor_to_types(form_factor: u32) -> (DeviceType, Option<InterfaceType>) {
    match form_factor {
        0 => (DeviceType::Unknown, Some(InterfaceType::Network)), // RemoteNetworkDevice
        1 => (DeviceType::Speaker, None),                         // Speakers
        2 => (DeviceType::Unknown, Some(InterfaceType::Line)),    // LineLevel
        3 => (DeviceType::Headphones, None),                      // Headphones
        4 => (DeviceType::Microphone, None),                      // Microphone
        5 => (DeviceType::Headset, None),                         // Headset
        6 => (DeviceType::Handset, None),                         // Handset
        7 => (DeviceType::Unknown, None),                         // UnknownDigitalPassthrough
        8 => (DeviceType::Unknown, Some(InterfaceType::Spdif)),   // SPDIF
        9 => (DeviceType::Unknown, Some(InterfaceType::Hdmi)),    // DigitalAudioDisplayDevice
        _ => (DeviceType::Unknown, None), // UnknownFormFactor or future values
    }
}

/// Maps WASAPI EnumeratorName to InterfaceType.
fn enumerator_to_interface_type(enumerator: &str) -> Option<InterfaceType> {
    let typ = match enumerator.to_uppercase().as_str() {
        "HDAUDIO" => InterfaceType::BuiltIn,
        "USB" => InterfaceType::Usb,
        "BTHENUM" => InterfaceType::Bluetooth,
        "MMDEVAPI" | "SW" => InterfaceType::Virtual,
        _ => return None,
    };
    Some(typ)
}

/// Activates an `IAudioClient` via `ActivateAudioInterfaceAsync` synchronously.
///
/// Used for virtual default-device GUIDs (`DEVINTERFACE_AUDIO_RENDER`/`DEVINTERFACE_AUDIO_CAPTURE`)
/// so that the Windows audio engine automatically reroutes the stream when the system default
/// device changes.
unsafe fn activate_audio_interface_sync(
    device_interface_path: windows::core::PWSTR,
    activation_timeout: Option<Duration>,
) -> windows::core::Result<Audio::IAudioClient> {
    use windows::core::IUnknown;

    #[windows::core::implement(Audio::IActivateAudioInterfaceCompletionHandler)]
    struct CompletionHandler(std::sync::mpsc::Sender<windows::core::Result<IUnknown>>);

    fn retrieve_result(
        operation: &Audio::IActivateAudioInterfaceAsyncOperation,
    ) -> windows::core::Result<IUnknown> {
        let mut result = windows::core::HRESULT::default();
        let mut interface: Option<IUnknown> = None;
        unsafe {
            operation.GetActivateResult(&mut result, &mut interface)?;
        }
        result.ok()?;
        interface.ok_or_else(|| {
            windows::core::Error::new(
                Audio::AUDCLNT_E_DEVICE_INVALIDATED,
                "audio interface not available after activation",
            )
        })
    }

    impl Audio::IActivateAudioInterfaceCompletionHandler_Impl for CompletionHandler_Impl {
        fn ActivateCompleted(
            &self,
            operation: windows::core::Ref<'_, Audio::IActivateAudioInterfaceAsyncOperation>,
        ) -> windows::core::Result<()> {
            let result = operation.ok().and_then(retrieve_result);
            let _ = self.0.send(result);
            Ok(())
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let handler: Audio::IActivateAudioInterfaceCompletionHandler = CompletionHandler(tx).into();
    Audio::ActivateAudioInterfaceAsync(
        device_interface_path,
        &Audio::IAudioClient::IID,
        None,
        &handler,
    )?;
    // If a timeout was given use it; otherwise block until Windows calls ActivateCompleted.
    // `handler` holds the sender and remains live on the stack, so `recv` cannot fail.
    let result = if let Some(dur) = activation_timeout {
        rx.recv_timeout(dur).map_err(|_| {
            windows::core::Error::new(
                ERROR_TIMEOUT.to_hresult(),
                "timeout waiting for audio interface activation",
            )
        })?
    } else {
        rx.recv().expect("activation channel closed; this is a bug")
    };
    result?.cast()
}

impl Device {
    pub fn description(&self) -> Result<DeviceDescription, Error> {
        let device = self.immdevice().ok_or_else(|| {
            Error::with_message(ErrorKind::DeviceNotAvailable, "default device not found")
        })?;
        unsafe {
            // Open the device's property store.
            let property_store = device
                .OpenPropertyStore(STGM_READ)
                .expect("could not open property store");

            // Query all available properties
            let friendly_name = get_property_string(
                &property_store,
                &Properties::DEVPKEY_Device_FriendlyName as *const _ as *const _,
            );

            let device_desc = get_property_string(
                &property_store,
                &Properties::DEVPKEY_Device_DeviceDesc as *const _ as *const _,
            );

            let interface_name = get_property_string(
                &property_store,
                &Properties::DEVPKEY_DeviceInterface_FriendlyName as *const _ as *const _,
            );

            let enumerator_name = get_property_string(
                &property_store,
                &Properties::DEVPKEY_Device_EnumeratorName as *const _ as *const _,
            );

            let form_factor = get_property_u32(
                &property_store,
                &PKEY_AUDIOENDPOINT_FORMFACTOR as *const _ as *const _,
            );

            let jack_subtype = get_property_string(
                &property_store,
                &PKEY_AUDIOENDPOINT_JACKSUBTYPE as *const _ as *const _,
            );

            // Prefer FriendlyName for name (e.g., "Speakers (XYZ Audio Adapter)"), fall back to DeviceDesc
            let name = friendly_name.or(device_desc).ok_or_else(|| {
                Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    "failed to retrieve device name",
                )
            })?;

            // Get direction from data flow (eCapture = Input, eRender = Output)
            let direction = self.data_flow().into();

            // Determine device_type and initial interface_type from FormFactor
            let (device_type, mut interface_type) = form_factor
                .map(form_factor_to_types)
                .unwrap_or((DeviceType::Unknown, None));

            // Override interface_type from EnumeratorName if available
            if let Some(ref enumerator) = enumerator_name {
                if let Some(itype) = enumerator_to_interface_type(enumerator) {
                    interface_type = Some(itype);
                }
            }

            // JackSubType has highest priority for interface_type
            if let Some(ref jack_guid) = jack_subtype {
                if let Some(itype) = jacksubtype_to_interface_type(jack_guid) {
                    interface_type = Some(itype);
                }
            }

            let mut builder = DeviceDescriptionBuilder::new(name)
                .direction(direction)
                .device_type(device_type);

            if let Some(itype) = interface_type {
                builder = builder.interface_type(itype);
            }

            // Add interface name to driver field if available
            if let Some(iface_name) = interface_name {
                builder = builder.driver(iface_name);
            }

            Ok(builder.build())
        }
    }

    fn id(&self) -> Result<DeviceId, Error> {
        let device = self.immdevice().ok_or_else(|| {
            Error::with_message(ErrorKind::DeviceNotAvailable, "default device not found")
        })?;
        unsafe {
            match device.GetId() {
                Ok(pwstr) => match pwstr.to_string() {
                    Ok(id_str) => Ok(DeviceId(crate::platform::HostId::Wasapi, id_str)),
                    Err(e) => Err(Error::with_message(
                        ErrorKind::Other,
                        format!("failed to convert device ID to string: {e}"),
                    )),
                },
                Err(e) => Err(Error::from(e)),
            }
        }
    }

    fn from_immdevice(device: Audio::IMMDevice) -> Self {
        Device {
            device: DeviceHandle::Specific(device),
            future_audio_client: Arc::new(Mutex::new(None)),
        }
    }

    fn default_output() -> Self {
        Device {
            device: DeviceHandle::DefaultOutput,
            future_audio_client: Arc::new(Mutex::new(None)),
        }
    }

    fn default_input() -> Self {
        Device {
            device: DeviceHandle::DefaultInput,
            future_audio_client: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the underlying `IMMDevice`, resolving the current one for default devices.
    pub fn immdevice(&self) -> Option<Audio::IMMDevice> {
        match &self.device {
            DeviceHandle::DefaultOutput => current_default_endpoint(Audio::eRender),
            DeviceHandle::DefaultInput => current_default_endpoint(Audio::eCapture),
            DeviceHandle::Specific(device) => Some(device.clone()),
        }
    }

    /// Creates a `DefaultDeviceMonitor` for default-device streams, or `None` for specific devices.
    fn default_device_monitor(&self) -> Result<Option<DefaultDeviceMonitor>, Error> {
        let flow = match &self.device {
            DeviceHandle::DefaultOutput => Audio::eRender,
            DeviceHandle::DefaultInput => Audio::eCapture,
            DeviceHandle::Specific(_) => return Ok(None),
        };
        let enumerator = get_enumerator().0.clone();
        DefaultDeviceMonitor::new(enumerator, flow).map(Some)
    }

    /// Ensures that `future_audio_client` contains a `Some` and returns a locked mutex to it.
    fn ensure_future_audio_client(
        &self,
        activation_timeout: Option<Duration>,
    ) -> Result<MutexGuard<'_, Option<IAudioClientWrapper>>, Error> {
        let mut lock = self.future_audio_client.lock().map_err(|_| {
            Error::with_message(ErrorKind::StreamInvalidated, "audio client lock poisoned")
        })?;
        if lock.is_some() {
            return Ok(lock);
        }

        let audio_client: Audio::IAudioClient = unsafe {
            match &self.device {
                DeviceHandle::DefaultOutput => {
                    let path = Com::StringFromIID(&Audio::DEVINTERFACE_AUDIO_RENDER)
                        .map_err(Error::from)?;
                    let _guard = ComString(path);
                    activate_audio_interface_sync(path, activation_timeout).map_err(Error::from)?
                }
                DeviceHandle::DefaultInput => {
                    let path = Com::StringFromIID(&Audio::DEVINTERFACE_AUDIO_CAPTURE)
                        .map_err(Error::from)?;
                    let _guard = ComString(path);
                    activate_audio_interface_sync(path, activation_timeout).map_err(Error::from)?
                }
                DeviceHandle::Specific(device) => {
                    // can fail if the device has been disconnected since we enumerated it, or if
                    // the device doesn't support playback for some reason
                    device
                        .Activate(Com::CLSCTX_ALL, None)
                        .map_err(Error::from)?
                }
            }
        };

        *lock = Some(IAudioClientWrapper(audio_client));
        Ok(lock)
    }

    /// Returns an uninitialized `IAudioClient`.
    pub(crate) fn build_audioclient(
        &self,
        activation_timeout: Option<Duration>,
    ) -> Result<Audio::IAudioClient, Error> {
        let mut lock = self.ensure_future_audio_client(activation_timeout)?;
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
    fn supported_formats(&self) -> Result<SupportedInputConfigs, Error> {
        // initializing COM because we call `CoTaskMemFree` to release the format.
        com::com_initialized();

        // Retrieve the `IAudioClient`.
        let lock = self
            .ensure_future_audio_client(None)
            .context("failed to get audio client")?;
        let client = &lock.as_ref().unwrap().0;

        unsafe {
            // Retrieve the pointer to the default WAVEFORMATEX.
            let default_waveformatex_ptr = client
                .GetMixFormat()
                .map(WaveFormatExPtr)
                .context("failed to get mix format")?;

            // If the default format can't succeed we have no hope of finding other formats.
            if !is_format_supported(client, default_waveformatex_ptr.0)? {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    "could not determine support for default WAVEFORMATEX",
                ));
            }

            let format = match format_from_waveformatex_ptr(default_waveformatex_ptr.0, client) {
                Some(fmt) => fmt,
                None => {
                    return Err(Error::with_message(
                        ErrorKind::UnsupportedConfig,
                        "could not create a SupportedStreamConfig from WAVEFORMATEX",
                    ));
                }
            };

            let mut sample_rates: Vec<SampleRate> = COMMON_SAMPLE_RATES.to_vec();

            if !sample_rates.contains(&format.sample_rate) {
                sample_rates.push(format.sample_rate)
            }

            let mut supported_formats = Vec::new();

            for sample_rate in sample_rates {
                for sample_format in [
                    SampleFormat::U8,
                    SampleFormat::I16,
                    SampleFormat::I24,
                    SampleFormat::U24,
                    SampleFormat::I32,
                    SampleFormat::I64,
                    SampleFormat::F32,
                ] {
                    if let Some(waveformat) = config_to_waveformatextensible(
                        StreamConfig {
                            channels: format.channels,
                            sample_rate,
                            buffer_size: BufferSize::Default,
                        },
                        sample_format,
                    ) {
                        if is_format_supported(
                            client,
                            &waveformat.Format as *const Audio::WAVEFORMATEX,
                        )? {
                            supported_formats.push(SupportedStreamConfigRange {
                                channels: format.channels,
                                min_sample_rate: sample_rate,
                                max_sample_rate: sample_rate,
                                buffer_size: format.buffer_size,
                                sample_format,
                            })
                        }
                    }
                }
            }
            Ok(supported_formats.into_iter())
        }
    }

    pub fn supported_input_configs(&self) -> Result<SupportedInputConfigs, Error> {
        if self.data_flow() == Audio::eCapture {
            self.supported_formats()
        // If it's an output device, assume no input formats.
        } else {
            Ok(vec![].into_iter())
        }
    }

    pub fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, Error> {
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
    fn default_format(&self) -> Result<SupportedStreamConfig, Error> {
        // initializing COM because we call `CoTaskMemFree`
        com::com_initialized();

        let lock = self
            .ensure_future_audio_client(None)
            .context("failed to get audio client")?;
        let client = &lock.as_ref().unwrap().0;

        unsafe {
            let format_ptr = client
                .GetMixFormat()
                .map(WaveFormatExPtr)
                .context("failed to get mix format")?;

            format_from_waveformatex_ptr(format_ptr.0, client).ok_or_else(|| {
                Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    "device audio format could not be mapped to a supported CPAL format",
                )
            })
        }
    }

    pub(crate) fn data_flow(&self) -> Audio::EDataFlow {
        match &self.device {
            DeviceHandle::DefaultOutput => Audio::eRender,
            DeviceHandle::DefaultInput => Audio::eCapture,
            DeviceHandle::Specific(device) => {
                let endpoint = Endpoint::from(device.clone());
                endpoint.data_flow()
            }
        }
    }

    pub fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        if self.data_flow() == Audio::eCapture {
            self.default_format()
        } else {
            Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "device does not support input",
            ))
        }
    }

    pub fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        let data_flow = self.data_flow();
        if data_flow == Audio::eRender {
            self.default_format()
        } else {
            Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "device does not support output",
            ))
        }
    }

    pub(crate) fn build_input_stream_raw_inner(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        activation_timeout: Option<Duration>,
    ) -> Result<StreamInner, Error> {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = self
                .build_audioclient(activation_timeout)
                .context("failed to build audio client")?;

            // Note: Buffer size validation is not needed here - `IAudioClient::Initialize`
            // will return `AUDCLNT_E_BUFFER_SIZE_ERROR` if the buffer size is not supported.
            let buffer_duration = buffer_size_to_duration(&config.buffer_size, config.sample_rate);

            let mut stream_flags = DEFAULT_FLAGS;

            if self.data_flow() == Audio::eRender {
                stream_flags |= Audio::AUDCLNT_STREAMFLAGS_LOOPBACK;
            }

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = config_to_waveformatextensible(config, sample_format)
                    .ok_or_else(|| {
                        Error::with_message(
                            ErrorKind::UnsupportedConfig,
                            "stream config could not be converted to a WASAPI-compatible format",
                        )
                    })?;
                let share_mode = Audio::AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(&audio_client, &format_attempt.Format) {
                    Ok(false) => {
                        return Err(Error::with_message(
                            ErrorKind::UnsupportedConfig,
                            "stream config is not supported by this WASAPI device in shared mode",
                        ))
                    }
                    Err(e) => return Err(e),
                    _ => (),
                }

                // Finally, initializing the audio client
                audio_client
                    .Initialize(
                        share_mode,
                        stream_flags,
                        buffer_duration,
                        0,
                        &format_attempt.Format,
                        None,
                    )
                    .context("failed to initialize audio client")?;

                format_attempt.Format
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = audio_client
                .GetBufferSize()
                .context("failed to get buffer size")?;

            let period_frames =
                shared_mode_period_frames(&audio_client, config.sample_rate, max_frames_in_buffer);

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event =
                Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))
                    .context("failed to create event")?;

            audio_client
                .SetEventHandle(event)
                .context("failed to set event handle")?;

            // Building a `IAudioCaptureClient` that will be used to read captured samples.
            let capture_client = audio_client
                .GetService::<Audio::IAudioCaptureClient>()
                .context("failed to get capture client")?;

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            let client_flow = AudioClientFlow::Capture { capture_client };

            let audio_clock = get_audio_clock(&audio_client)?;

            let stream_latency = {
                let hns = audio_client
                    .GetStreamLatency()
                    .context("failed to get stream latency")?;
                Duration::from_nanos(hns.max(0) as u64 * 100)
            };

            Ok(StreamInner {
                audio_client,
                audio_clock,
                client_flow,
                event,
                playing: false,
                max_frames_in_buffer,
                period_frames,
                bytes_per_frame: waveformatex.nBlockAlign,
                config,
                sample_format,
                stream_latency,
            })
        }
    }

    pub(crate) fn build_output_stream_raw_inner(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        activation_timeout: Option<Duration>,
    ) -> Result<StreamInner, Error> {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = self
                .build_audioclient(activation_timeout)
                .context("failed to build audio client")?;

            // Note: Buffer size validation is not needed here - `IAudioClient::Initialize`
            // will return `AUDCLNT_E_BUFFER_SIZE_ERROR` if the buffer size is not supported.
            let buffer_duration = buffer_size_to_duration(&config.buffer_size, config.sample_rate);

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = config_to_waveformatextensible(config, sample_format)
                    .ok_or_else(|| {
                        Error::with_message(
                            ErrorKind::UnsupportedConfig,
                            "stream config could not be converted to a WASAPI-compatible format",
                        )
                    })?;
                let share_mode = Audio::AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(&audio_client, &format_attempt.Format) {
                    Ok(false) => {
                        return Err(Error::with_message(
                            ErrorKind::UnsupportedConfig,
                            "stream config is not supported by this WASAPI device in shared mode",
                        ))
                    }
                    Err(e) => return Err(e),
                    _ => (),
                }

                // Finally, initializing the audio client
                audio_client
                    .Initialize(
                        share_mode,
                        DEFAULT_FLAGS,
                        buffer_duration,
                        0,
                        &format_attempt.Format,
                        None,
                    )
                    .context("failed to initialize audio client")?;

                format_attempt.Format
            };

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event =
                Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))
                    .context("failed to create event")?;

            audio_client
                .SetEventHandle(event)
                .context("failed to set event handle")?;

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = audio_client
                .GetBufferSize()
                .context("failed to get buffer size")?;

            let period_frames =
                shared_mode_period_frames(&audio_client, config.sample_rate, max_frames_in_buffer);

            // Building a `IAudioRenderClient` that will be used to fill the samples buffer.
            let render_client = audio_client
                .GetService::<IAudioRenderClient>()
                .context("failed to get render client")?;

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            let client_flow = AudioClientFlow::Render { render_client };

            let audio_clock = get_audio_clock(&audio_client)?;

            let stream_latency = {
                let hns = audio_client
                    .GetStreamLatency()
                    .context("failed to get stream latency")?;
                Duration::from_nanos(hns.max(0) as u64 * 100)
            };

            Ok(StreamInner {
                audio_client,
                audio_clock,
                client_flow,
                event,
                playing: false,
                max_frames_in_buffer,
                period_frames,
                bytes_per_frame: waveformatex.nBlockAlign,
                config,
                sample_format,
                stream_latency,
            })
        }
    }
}

/// Compares the endpoint IDs of two `IMMDevice` objects.
///
/// # Safety
///
/// Both devices must be valid, live `IMMDevice` COM objects.
unsafe fn endpoint_ids_equal(a: &Audio::IMMDevice, b: &Audio::IMMDevice) -> bool {
    let id_a = a.GetId().expect("cpal: GetId failure");
    let id_b = b.GetId().expect("cpal: GetId failure");
    let _ga = ComString(id_a);
    let _gb = ComString(id_b);
    let mut off = 0isize;
    loop {
        let wa = *id_a.0.offset(off);
        let wb = *id_b.0.offset(off);
        if wa != wb {
            return false;
        }
        if wa == 0 {
            return true;
        }
        off += 1;
    }
}

/// Hashes the endpoint ID of an `IMMDevice` into `state` without allocating.
///
/// # Safety
/// `device` must be a valid, live `IMMDevice` COM object.
unsafe fn hash_endpoint_id<H: std::hash::Hasher>(device: &Audio::IMMDevice, state: &mut H) {
    let id = device.GetId().expect("cpal: GetId failure");
    let _g = ComString(id);
    let mut off = 0isize;
    loop {
        let w = *id.0.offset(off);
        if w == 0 {
            break;
        }
        w.hash(state);
        off += 1;
    }
}

// Equality and hashing use stable identifiers only.
impl PartialEq for Device {
    fn eq(&self, other: &Device) -> bool {
        match (&self.device, &other.device) {
            (DeviceHandle::DefaultOutput, DeviceHandle::DefaultOutput)
            | (DeviceHandle::DefaultInput, DeviceHandle::DefaultInput) => true,
            (DeviceHandle::Specific(a), DeviceHandle::Specific(b)) => {
                // SAFETY: both IMMDevice handles are valid for the lifetime of their Device.
                unsafe { endpoint_ids_equal(a, b) }
            }
            _ => false,
        }
    }
}

impl Eq for Device {}

impl std::hash::Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match &self.device {
            DeviceHandle::DefaultOutput | DeviceHandle::DefaultInput => {
                mem::discriminant(&self.device).hash(state);
            }
            DeviceHandle::Specific(device) => {
                // SAFETY: the IMMDevice handle is valid for the Device's lifetime.
                unsafe { hash_endpoint_id(device, state) }
            }
        }
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("device", &self.device)
            .field("description", &self.description())
            .finish()
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
        unsafe { data_flow_from_immendpoint(&self.endpoint) }
    }
}

static ENUMERATOR: OnceLock<Enumerator> = OnceLock::new();

/// Returns the current default audio endpoint for `flow`, or `None` if none exists.
///
/// Shared by [`Device::immdevice`] and the stream-side `get_current_default` helper to
/// avoid duplicating the `GetDefaultAudioEndpoint` call.
pub(super) fn current_default_endpoint(flow: Audio::EDataFlow) -> Option<Audio::IMMDevice> {
    // Ensure COM is initialised on this thread — callers include notification callbacks on
    // threads that may not have called CoInitialize themselves.
    com::com_initialized();
    // SAFETY: `get_enumerator()` is a thread-safe singleton initialised at first use.
    unsafe {
        get_enumerator()
            .0
            .GetDefaultAudioEndpoint(flow, Audio::eConsole)
            .ok()
    }
}

fn get_enumerator() -> &'static Enumerator {
    ENUMERATOR.get_or_init(|| {
        // COM initialization is thread local, but we only need to have COM initialized in the
        // thread we create the objects in
        com::com_initialized();

        // building the devices enumerator object
        unsafe {
            let enumerator = Com::CoCreateInstance::<_, Audio::IMMDeviceEnumerator>(
                &Audio::MMDeviceEnumerator,
                None,
                Com::CLSCTX_ALL,
            )
            .unwrap();

            Enumerator(enumerator)
        }
    })
}

// Helper function to query a DWORD property from a WASAPI device property store
unsafe fn get_property_u32(
    property_store: &IPropertyStore,
    property_key: *const PROPERTYKEY,
) -> Option<u32> {
    let mut property_value = property_store.GetValue(property_key).ok()?;
    let prop_variant = &property_value.Anonymous.Anonymous;

    // Check if it's a UI4 (unsigned 32-bit integer)
    if prop_variant.vt != VT_UI4 {
        return None;
    }

    let value = *(&prop_variant.Anonymous as *const _ as *const u32);

    // Clean up the property
    StructuredStorage::PropVariantClear(&mut property_value).ok();

    Some(value)
}

// Helper function to query a string property from a WASAPI device property store
unsafe fn get_property_string(
    property_store: &IPropertyStore,
    property_key: *const PROPERTYKEY,
) -> Option<String> {
    let mut property_value = property_store.GetValue(property_key).ok()?;
    let prop_variant = &property_value.Anonymous.Anonymous;

    // Read the string from the union data field, expecting a *const u16.
    if prop_variant.vt != VT_LPWSTR {
        return None;
    }
    let ptr_utf16 = *(&prop_variant.Anonymous as *const _ as *const *const u16);

    // Find the length of the null-terminated string with a safety limit
    const MAX_STRING_LEN: usize = 32768; // 32K characters should be more than enough
    let mut len = 0;
    while len < MAX_STRING_LEN && *ptr_utf16.add(len) != 0 {
        len += 1;
    }

    // If we hit the limit, the string is likely malformed (not null-terminated)
    if len >= MAX_STRING_LEN {
        return None;
    }

    // Create the utf16 slice and convert it into a string.
    let string_slice = slice::from_raw_parts(ptr_utf16, len);
    let os_string: OsString = OsStringExt::from_wide(string_slice);
    let result = match os_string.into_string() {
        Ok(string) => Some(string),
        Err(os_string) => Some(os_string.to_string_lossy().into()),
    };

    // Clean up the property.
    StructuredStorage::PropVariantClear(&mut property_value).ok();

    result
}

/// Send/Sync wrapper around `IMMDeviceEnumerator`.
struct Enumerator(Audio::IMMDeviceEnumerator);

unsafe impl Send for Enumerator {}
unsafe impl Sync for Enumerator {}

/// WASAPI implementation for `Devices`.
pub struct Devices {
    collection: Audio::IMMDeviceCollection,
    total_count: u32,
    next_item: u32,
}

impl Devices {
    pub fn new() -> Result<Self, Error> {
        unsafe {
            // can fail because of wrong parameters (should never happen) or out of memory
            let collection = get_enumerator()
                .0
                .EnumAudioEndpoints(Audio::eAll, Audio::DEVICE_STATE_ACTIVE)
                .context("failed to enumerate audio endpoints")?;

            let count = collection
                .GetCount()
                .context("failed to get device count")?;

            Ok(Self {
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

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let device = self.collection.Item(self.next_item).unwrap();
            self.next_item += 1;
            Some(Self::Item::from_immdevice(device))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let num = self.total_count - self.next_item;
        let num = num as usize;
        (num, Some(num))
    }
}

pub fn default_input_device() -> Option<Device> {
    // Detect if a default input device exists before creating a `Device` for it.
    current_default_endpoint(Audio::eCapture).map(|_| Device::default_input())
}

pub fn default_output_device() -> Option<Device> {
    // Detect if a default output device exists before creating a `Device` for it.
    current_default_endpoint(Audio::eRender).map(|_| Device::default_output())
}

/// Get the audio clock used to produce `StreamInstant`s.
unsafe fn get_audio_clock(audio_client: &Audio::IAudioClient) -> Result<Audio::IAudioClock, Error> {
    audio_client
        .GetService::<Audio::IAudioClock>()
        .context("failed to get audio clock")
}

// Turns a `Format` into a `WAVEFORMATEXTENSIBLE`.
//
// Returns `None` if the WAVEFORMATEXTENSIBLE does not support the given format.
fn config_to_waveformatextensible(
    config: StreamConfig,
    sample_format: SampleFormat,
) -> Option<Audio::WAVEFORMATEXTENSIBLE> {
    let format_tag = match sample_format {
        SampleFormat::U8 | SampleFormat::I16 => Audio::WAVE_FORMAT_PCM,

        SampleFormat::I24
        | SampleFormat::U24
        | SampleFormat::I32
        | SampleFormat::I64
        | SampleFormat::F32 => KernelStreaming::WAVE_FORMAT_EXTENSIBLE,

        _ => return None,
    };
    let channels = config.channels;
    let sample_rate = config.sample_rate;
    let sample_bytes = sample_format.sample_size() as u16;
    let avg_bytes_per_sec = u32::from(channels) * sample_rate * u32::from(sample_bytes);
    let block_align = channels * sample_bytes;
    let bits_per_sample = match sample_format {
        // 24-bit formats use 32-bit storage but only 24 valid bits
        SampleFormat::I24 | SampleFormat::U24 => 24,
        _ => 8 * sample_bytes,
    };

    let cb_size = if format_tag == Audio::WAVE_FORMAT_PCM {
        0
    } else {
        let extensible_size = mem::size_of::<Audio::WAVEFORMATEXTENSIBLE>();
        let ex_size = mem::size_of::<Audio::WAVEFORMATEX>();
        (extensible_size - ex_size) as u16
    };

    let waveformatex = Audio::WAVEFORMATEX {
        wFormatTag: format_tag as u16,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: avg_bytes_per_sec,
        nBlockAlign: block_align,
        wBitsPerSample: bits_per_sample,
        cbSize: cb_size,
    };

    // CPAL does not care about speaker positions, so pass audio right through.
    let channel_mask = KernelStreaming::KSAUDIO_SPEAKER_DIRECTOUT;

    let sub_format = match sample_format {
        SampleFormat::U8
        | SampleFormat::I16
        | SampleFormat::I24
        | SampleFormat::U24
        | SampleFormat::I32
        | SampleFormat::I64 => KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM,

        SampleFormat::F32 => Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        _ => return None,
    };

    let waveformatextensible = Audio::WAVEFORMATEXTENSIBLE {
        Format: waveformatex,
        Samples: Audio::WAVEFORMATEXTENSIBLE_0 {
            wSamplesPerBlock: bits_per_sample,
        },
        dwChannelMask: channel_mask,
        SubFormat: sub_format,
    };

    Some(waveformatextensible)
}

/// Get the default device period in frames for a shared-mode stream.
fn shared_mode_period_frames(
    audio_client: &Audio::IAudioClient,
    sample_rate: SampleRate,
    max_frames_in_buffer: FrameCount,
) -> FrameCount {
    let mut default_period = 0i64;
    if unsafe { audio_client.GetDevicePeriod(Some(&mut default_period), None) }.is_ok()
        && default_period > 0
    {
        buffer_duration_to_frames(default_period, sample_rate)
    } else {
        max_frames_in_buffer
    }
}

fn buffer_size_to_duration(buffer_size: &BufferSize, sample_rate: SampleRate) -> i64 {
    match buffer_size {
        BufferSize::Fixed(frames) => *frames as i64 * (1_000_000_000 / 100) / sample_rate as i64,
        BufferSize::Default => 0,
    }
}

fn buffer_duration_to_frames(buffer_duration: i64, sample_rate: SampleRate) -> FrameCount {
    (buffer_duration * sample_rate as i64 * 100 / 1_000_000_000) as FrameCount
}

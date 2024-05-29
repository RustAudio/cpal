extern crate core_foundation_sys;
extern crate coreaudio;

use super::{asbd_from_config, check_os_status, frames_to_duration, host_time_to_stream_instant};

use self::core_foundation_sys::string::{CFStringGetCString, CFStringGetCStringPtr, CFStringRef};
use self::coreaudio::audio_unit::render_callback::{self, data};
use self::coreaudio::audio_unit::{AudioUnit, Element, Scope};
use self::coreaudio::sys::{
    kAudioDevicePropertyAvailableNominalSampleRates, kAudioDevicePropertyBufferFrameSize,
    kAudioDevicePropertyBufferFrameSizeRange, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyDeviceNameCFString, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertyScopeOutput, kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreamFormat, kAudioObjectPropertyElementMaster,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput,
    kAudioObjectPropertyScopeOutput, kAudioOutputUnitProperty_CurrentDevice,
    kAudioOutputUnitProperty_EnableIO, kAudioUnitProperty_StreamFormat, kCFStringEncodingUTF8,
    AudioBuffer, AudioBufferList, AudioDeviceID, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectPropertyScope, AudioObjectSetPropertyData, AudioStreamBasicDescription,
    AudioValueRange, OSStatus,
};
use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, ChannelCount, Data,
    DefaultStreamConfigError, DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo,
    PauseStreamError, PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};
use std::ffi::CStr;
use std::fmt;
use std::mem;
use std::os::raw::c_char;
use std::ptr::null;
use std::rc::Rc;
use std::slice;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use self::enumerate::{
    default_input_device, default_output_device, Devices, SupportedInputConfigs,
    SupportedOutputConfigs,
};

use property_listener::AudioObjectPropertyListener;

pub mod enumerate;
mod property_listener;

/// Coreaudio host, the default host on macOS.
#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume coreaudio is always available
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }
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
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Device::build_input_stream_raw(
            self,
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Device::build_output_stream_raw(
            self,
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Device {
    pub(crate) audio_device_id: AudioDeviceID,
    is_default: bool,
}

impl Device {
    fn name(&self) -> Result<String, DeviceNameError> {
        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyDeviceNameCFString,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMaster,
        };
        let device_name: CFStringRef = null();
        let data_size = mem::size_of::<CFStringRef>();
        let c_str = unsafe {
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                &device_name as *const _ as *mut _,
            );
            check_os_status(status)?;

            let c_string: *const c_char = CFStringGetCStringPtr(device_name, kCFStringEncodingUTF8);
            if c_string.is_null() {
                let status = AudioObjectGetPropertyData(
                    self.audio_device_id,
                    &property_address as *const _,
                    0,
                    null(),
                    &data_size as *const _ as *mut _,
                    &device_name as *const _ as *mut _,
                );
                check_os_status(status)?;
                let mut buf: [i8; 255] = [0; 255];
                let result = CFStringGetCString(
                    device_name,
                    buf.as_mut_ptr(),
                    buf.len() as _,
                    kCFStringEncodingUTF8,
                );
                if result == 0 {
                    let description =
                        "core foundation failed to return device name string".to_string();
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
                let name: &CStr = CStr::from_ptr(buf.as_ptr());
                return Ok(name.to_str().unwrap().to_owned());
            }
            CStr::from_ptr(c_string as *mut _)
        };
        Ok(c_str.to_string_lossy().into_owned())
    }

    // Logic re-used between `supported_input_configs` and `supported_output_configs`.
    #[allow(clippy::cast_ptr_alignment)]
    fn supported_configs(
        &self,
        scope: AudioObjectPropertyScope,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        let mut property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMaster,
        };

        unsafe {
            // Retrieve the devices audio buffer list.
            let data_size = 0u32;
            let status = AudioObjectGetPropertyDataSize(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
            );
            check_os_status(status)?;

            let mut audio_buffer_list: Vec<u8> = vec![];
            audio_buffer_list.reserve_exact(data_size as usize);
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                audio_buffer_list.as_mut_ptr() as *mut _,
            );
            check_os_status(status)?;

            let audio_buffer_list = audio_buffer_list.as_mut_ptr() as *mut AudioBufferList;

            // If there's no buffers, skip.
            if (*audio_buffer_list).mNumberBuffers == 0 {
                return Ok(vec![].into_iter());
            }

            // Count the number of channels as the sum of all channels in all output buffers.
            let n_buffers = (*audio_buffer_list).mNumberBuffers as usize;
            let first: *const AudioBuffer = (*audio_buffer_list).mBuffers.as_ptr();
            let buffers: &'static [AudioBuffer] = slice::from_raw_parts(first, n_buffers);
            let mut n_channels = 0;
            for buffer in buffers {
                n_channels += buffer.mNumberChannels as usize;
            }

            // TODO: macOS should support U8, I16, I32, F32 and F64. This should allow for using
            // I16 but just use F32 for now as it's the default anyway.
            let sample_format = SampleFormat::F32;

            // Get available sample rate ranges.
            property_address.mSelector = kAudioDevicePropertyAvailableNominalSampleRates;
            let data_size = 0u32;
            let status = AudioObjectGetPropertyDataSize(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
            );
            check_os_status(status)?;

            let n_ranges = data_size as usize / mem::size_of::<AudioValueRange>();
            let mut ranges: Vec<u8> = vec![];
            ranges.reserve_exact(data_size as usize);
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                ranges.as_mut_ptr() as *mut _,
            );
            check_os_status(status)?;

            let ranges: *mut AudioValueRange = ranges.as_mut_ptr() as *mut _;
            let ranges: &'static [AudioValueRange] = slice::from_raw_parts(ranges, n_ranges);

            let audio_unit = audio_unit_from_device(self, true)?;
            let buffer_size = get_io_buffer_frame_size_range(&audio_unit)?;

            // Collect the supported formats for the device.
            let mut fmts = vec![];
            for range in ranges {
                let fmt = SupportedStreamConfigRange {
                    channels: n_channels as ChannelCount,
                    min_sample_rate: SampleRate(range.mMinimum as _),
                    max_sample_rate: SampleRate(range.mMaximum as _),
                    buffer_size,
                    sample_format,
                };
                fmts.push(fmt);
            }

            Ok(fmts.into_iter())
        }
    }

    fn supported_input_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        self.supported_configs(kAudioObjectPropertyScopeInput)
    }

    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        self.supported_configs(kAudioObjectPropertyScopeOutput)
    }

    fn default_config(
        &self,
        scope: AudioObjectPropertyScope,
    ) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        fn default_config_error_from_os_status(
            status: OSStatus,
        ) -> Result<(), DefaultStreamConfigError> {
            let err = match coreaudio::Error::from_os_status(status) {
                Err(err) => err,
                Ok(_) => return Ok(()),
            };
            match err {
                coreaudio::Error::AudioUnit(
                    coreaudio::error::AudioUnitError::FormatNotSupported,
                )
                | coreaudio::Error::AudioCodec(_)
                | coreaudio::Error::AudioFormat(_) => {
                    Err(DefaultStreamConfigError::StreamTypeNotSupported)
                }
                coreaudio::Error::AudioUnit(coreaudio::error::AudioUnitError::NoConnection) => {
                    Err(DefaultStreamConfigError::DeviceNotAvailable)
                }
                err => {
                    let description = format!("{}", err);
                    let err = BackendSpecificError { description };
                    Err(err.into())
                }
            }
        }

        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamFormat,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMaster,
        };

        unsafe {
            let asbd: AudioStreamBasicDescription = mem::zeroed();
            let data_size = mem::size_of::<AudioStreamBasicDescription>() as u32;
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                &asbd as *const _ as *mut _,
            );
            default_config_error_from_os_status(status)?;

            let sample_format = {
                let audio_format = coreaudio::audio_unit::AudioFormat::from_format_and_flag(
                    asbd.mFormatID,
                    Some(asbd.mFormatFlags),
                );
                let flags = match audio_format {
                    Some(coreaudio::audio_unit::AudioFormat::LinearPCM(flags)) => flags,
                    _ => return Err(DefaultStreamConfigError::StreamTypeNotSupported),
                };
                let maybe_sample_format =
                    coreaudio::audio_unit::SampleFormat::from_flags_and_bits_per_sample(
                        flags,
                        asbd.mBitsPerChannel,
                    );
                match maybe_sample_format {
                    Some(coreaudio::audio_unit::SampleFormat::F32) => SampleFormat::F32,
                    Some(coreaudio::audio_unit::SampleFormat::I16) => SampleFormat::I16,
                    _ => return Err(DefaultStreamConfigError::StreamTypeNotSupported),
                }
            };

            let audio_unit = audio_unit_from_device(self, true)?;
            let buffer_size = get_io_buffer_frame_size_range(&audio_unit)?;

            let config = SupportedStreamConfig {
                sample_rate: SampleRate(asbd.mSampleRate as _),
                channels: asbd.mChannelsPerFrame as _,
                buffer_size,
                sample_format,
            };
            Ok(config)
        }
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(kAudioObjectPropertyScopeInput)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(kAudioObjectPropertyScopeOutput)
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("audio_device_id", &self.audio_device_id)
            .field("name", &self.name())
            .finish()
    }
}

struct StreamInner {
    playing: bool,
    audio_unit: AudioUnit,
    /// Manage the lifetime of the closure that handles device disconnection.
    _disconnect_listener: Option<AudioObjectPropertyListener>,
    // Track the device with which the audio unit was spawned.
    //
    // We must do this so that we can avoid changing the device sample rate if there is already
    // a stream associated with the device.
    #[allow(dead_code)]
    device_id: AudioDeviceID,
}

/// Register the on-disconnect callback.
/// This will both stop the stream and call the error callback with DeviceNotAvailable.
/// This function should only be called once per stream.
fn add_disconnect_listener<E>(
    stream: &Stream,
    error_callback: Arc<Mutex<E>>,
) -> Result<(), BuildStreamError>
where
    E: FnMut(StreamError) + Send + 'static,
{
    let stream_copy = stream.clone();
    let mut stream_inner = stream.inner.lock().unwrap();
    stream_inner._disconnect_listener = Some(AudioObjectPropertyListener::new(
        stream_inner.device_id,
        AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyDeviceIsAlive,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        },
        move || {
            let _ = stream_copy.pause();
            (error_callback.lock().unwrap())(StreamError::DeviceNotAvailable);
        },
    )?);
    Ok(())
}

fn audio_unit_from_device(device: &Device, input: bool) -> Result<AudioUnit, coreaudio::Error> {
    let output_type = if device.is_default && !input {
        coreaudio::audio_unit::IOType::DefaultOutput
    } else {
        coreaudio::audio_unit::IOType::HalOutput
    };
    let mut audio_unit = AudioUnit::new(output_type)?;

    if input {
        // Enable input processing.
        let enable_input = 1u32;
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Input,
            Element::Input,
            Some(&enable_input),
        )?;

        // Disable output processing.
        let disable_output = 0u32;
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Output,
            Element::Output,
            Some(&disable_output),
        )?;
    }

    audio_unit.set_property(
        kAudioOutputUnitProperty_CurrentDevice,
        Scope::Global,
        Element::Output,
        Some(&device.audio_device_id),
    )?;

    Ok(audio_unit)
}

impl Device {
    #[allow(clippy::cast_ptr_alignment)]
    #[allow(clippy::while_immutable_condition)]
    #[allow(clippy::float_cmp)]
    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // The scope and element for working with a device's input stream.
        let scope = Scope::Output;
        let element = Element::Input;

        // Potentially change the device sample rate to match the config.
        set_sample_rate(self.audio_device_id, config.sample_rate)?;

        let mut audio_unit = audio_unit_from_device(self, true)?;

        // Set the stream in interleaved mode.
        let asbd = asbd_from_config(config, sample_format);
        audio_unit.set_property(kAudioUnitProperty_StreamFormat, scope, element, Some(&asbd))?;

        // Set the buffersize
        match config.buffer_size {
            BufferSize::Fixed(v) => {
                let buffer_size_range = get_io_buffer_frame_size_range(&audio_unit)?;
                match buffer_size_range {
                    SupportedBufferSize::Range { min, max } => {
                        if v >= min && v <= max {
                            audio_unit.set_property(
                                kAudioDevicePropertyBufferFrameSize,
                                scope,
                                element,
                                Some(&v),
                            )?
                        } else {
                            return Err(BuildStreamError::StreamConfigNotSupported);
                        }
                    }
                    SupportedBufferSize::Unknown => (),
                }
            }
            BufferSize::Default => (),
        }

        let error_callback = Arc::new(Mutex::new(error_callback));
        let error_callback_disconnect = error_callback.clone();

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let bytes_per_channel = sample_format.sample_size();
        let sample_rate = config.sample_rate;
        type Args = render_callback::Args<data::Raw>;
        audio_unit.set_input_callback(move |args: Args| unsafe {
            let ptr = (*args.data.data).mBuffers.as_ptr();
            let len = (*args.data.data).mNumberBuffers as usize;
            let buffers: &[AudioBuffer] = slice::from_raw_parts(ptr, len);

            // TODO: Perhaps loop over all buffers instead?
            let AudioBuffer {
                mNumberChannels: channels,
                mDataByteSize: data_byte_size,
                mData: data,
            } = buffers[0];

            let data = data as *mut ();
            let len = data_byte_size as usize / bytes_per_channel;
            let data = Data::from_parts(data, len, sample_format);

            // TODO: Need a better way to get delay, for now we assume a double-buffer offset.
            let callback = match host_time_to_stream_instant(args.time_stamp.mHostTime) {
                Err(err) => {
                    (error_callback.lock().unwrap())(err.into());
                    return Err(());
                }
                Ok(cb) => cb,
            };
            let buffer_frames = len / channels as usize;
            let delay = frames_to_duration(buffer_frames, sample_rate);
            let capture = callback
                .sub(delay)
                .expect("`capture` occurs before origin of alsa `StreamInstant`");
            let timestamp = crate::InputStreamTimestamp { callback, capture };

            let info = InputCallbackInfo { timestamp };
            data_callback(&data, &info);
            Ok(())
        })?;

        let stream = Stream::new(StreamInner {
            playing: true,
            _disconnect_listener: None,
            audio_unit,
            device_id: self.audio_device_id,
        });

        // If we didn't request the default device, stop the stream if the
        // device disconnects.
        if !self.is_default {
            add_disconnect_listener(&stream, error_callback_disconnect)?;
        }

        stream.inner.lock().unwrap().audio_unit.start()?;

        Ok(stream)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let mut audio_unit = audio_unit_from_device(self, false)?;

        // The scope and element for working with a device's output stream.
        let scope = Scope::Input;
        let element = Element::Output;

        // Set the stream in interleaved mode.
        let asbd = asbd_from_config(config, sample_format);
        audio_unit.set_property(kAudioUnitProperty_StreamFormat, scope, element, Some(&asbd))?;

        // Set the buffersize
        match config.buffer_size {
            BufferSize::Fixed(v) => {
                let buffer_size_range = get_io_buffer_frame_size_range(&audio_unit)?;
                match buffer_size_range {
                    SupportedBufferSize::Range { min, max } => {
                        if v >= min && v <= max {
                            audio_unit.set_property(
                                kAudioDevicePropertyBufferFrameSize,
                                scope,
                                element,
                                Some(&v),
                            )?
                        } else {
                            return Err(BuildStreamError::StreamConfigNotSupported);
                        }
                    }
                    SupportedBufferSize::Unknown => (),
                }
            }
            BufferSize::Default => (),
        }

        let error_callback = Arc::new(Mutex::new(error_callback));
        let error_callback_disconnect = error_callback.clone();

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let bytes_per_channel = sample_format.sample_size();
        let sample_rate = config.sample_rate;
        type Args = render_callback::Args<data::Raw>;
        audio_unit.set_render_callback(move |args: Args| unsafe {
            // If `run()` is currently running, then a callback will be available from this list.
            // Otherwise, we just fill the buffer with zeroes and return.

            let AudioBuffer {
                mNumberChannels: channels,
                mDataByteSize: data_byte_size,
                mData: data,
            } = (*args.data.data).mBuffers[0];

            let data = data as *mut ();
            let len = data_byte_size as usize / bytes_per_channel;
            let mut data = Data::from_parts(data, len, sample_format);

            let callback = match host_time_to_stream_instant(args.time_stamp.mHostTime) {
                Err(err) => {
                    (error_callback.lock().unwrap())(err.into());
                    return Err(());
                }
                Ok(cb) => cb,
            };
            // TODO: Need a better way to get delay, for now we assume a double-buffer offset.
            let buffer_frames = len / channels as usize;
            let delay = frames_to_duration(buffer_frames, sample_rate);
            let playback = callback
                .add(delay)
                .expect("`playback` occurs beyond representation supported by `StreamInstant`");
            let timestamp = crate::OutputStreamTimestamp { callback, playback };

            let info = OutputCallbackInfo { timestamp };
            data_callback(&mut data, &info);
            Ok(())
        })?;

        let stream = Stream::new(StreamInner {
            playing: true,
            _disconnect_listener: None,
            audio_unit,
            device_id: self.audio_device_id,
        });

        // If we didn't request the default device, stop the stream if the
        // device disconnects.
        if !self.is_default {
            add_disconnect_listener(&stream, error_callback_disconnect)?;
        }

        stream.inner.lock().unwrap().audio_unit.start()?;

        Ok(stream)
    }
}

/// Attempt to set the device sample rate to the provided rate.
/// Return an error if the requested sample rate is not supported by the device.
fn set_sample_rate(
    audio_device_id: AudioObjectID,
    target_sample_rate: SampleRate,
) -> Result<(), BuildStreamError> {
    // Get the current sample rate.
    let mut property_address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyNominalSampleRate,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let sample_rate: f64 = 0.0;
    let data_size = mem::size_of::<f64>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            audio_device_id,
            &property_address as *const _,
            0,
            null(),
            &data_size as *const _ as *mut _,
            &sample_rate as *const _ as *mut _,
        )
    };
    coreaudio::Error::from_os_status(status)?;

    // If the requested sample rate is different to the device sample rate, update the device.
    if sample_rate as u32 != target_sample_rate.0 {
        // Get available sample rate ranges.
        property_address.mSelector = kAudioDevicePropertyAvailableNominalSampleRates;
        let data_size = 0u32;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
            )
        };
        coreaudio::Error::from_os_status(status)?;
        let n_ranges = data_size as usize / mem::size_of::<AudioValueRange>();
        let mut ranges: Vec<u8> = vec![];
        ranges.reserve_exact(data_size as usize);
        let status = unsafe {
            AudioObjectGetPropertyData(
                audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                ranges.as_mut_ptr() as *mut _,
            )
        };
        coreaudio::Error::from_os_status(status)?;
        let ranges: *mut AudioValueRange = ranges.as_mut_ptr() as *mut _;
        let ranges: &'static [AudioValueRange] = unsafe { slice::from_raw_parts(ranges, n_ranges) };

        // Now that we have the available ranges, pick the one matching the desired rate.
        let sample_rate = target_sample_rate.0;
        let maybe_index = ranges
            .iter()
            .position(|r| r.mMinimum as u32 == sample_rate && r.mMaximum as u32 == sample_rate);
        let range_index = match maybe_index {
            None => return Err(BuildStreamError::StreamConfigNotSupported),
            Some(i) => i,
        };

        let (send, recv) = channel::<Result<f64, coreaudio::Error>>();
        let sample_rate_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyNominalSampleRate,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        // Send sample rate updates back on a channel.
        let sample_rate_handler = move || {
            let mut rate: f64 = 0.0;
            let data_size = mem::size_of::<f64>();

            let result = unsafe {
                AudioObjectGetPropertyData(
                    audio_device_id,
                    &sample_rate_address as *const _,
                    0,
                    null(),
                    &data_size as *const _ as *mut _,
                    &mut rate as *const _ as *mut _,
                )
            };
            send.send(coreaudio::Error::from_os_status(result).map(|_| rate))
                .ok();
        };

        let listener = AudioObjectPropertyListener::new(
            audio_device_id,
            sample_rate_address,
            sample_rate_handler,
        )?;

        // Finally, set the sample rate.
        property_address.mSelector = kAudioDevicePropertyNominalSampleRate;
        let status = unsafe {
            AudioObjectSetPropertyData(
                audio_device_id,
                &property_address as *const _,
                0,
                null(),
                data_size,
                &ranges[range_index] as *const _ as *const _,
            )
        };
        coreaudio::Error::from_os_status(status)?;

        // Wait for the reported_rate to change.
        //
        // This should not take longer than a few ms, but we timeout after 1 sec just in case.
        // We loop over potentially several events from the channel to ensure
        // that we catch the expected change in sample rate.
        let mut timeout = Duration::from_secs(1);
        let start = Instant::now();

        loop {
            match recv.recv_timeout(timeout) {
                Err(err) => {
                    let description = match err {
                        RecvTimeoutError::Disconnected => {
                            "sample rate listener channel disconnected unexpectedly"
                        }
                        RecvTimeoutError::Timeout => {
                            "timeout waiting for sample rate update for device"
                        }
                    }
                    .to_string();
                    return Err(BackendSpecificError { description }.into());
                }
                Ok(Ok(reported_sample_rate)) => {
                    if reported_sample_rate == target_sample_rate.0 as f64 {
                        break;
                    }
                }
                Ok(Err(_)) => {
                    // TODO: should we consider collecting this error?
                }
            };
            timeout = timeout
                .checked_sub(start.elapsed())
                .unwrap_or(Duration::ZERO);
        }
        listener.remove()?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct Stream {
    inner: Arc<Mutex<StreamInner>>,
}

impl Stream {
    fn new(inner: StreamInner) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let mut stream = self.inner.lock().unwrap();

        if !stream.playing {
            if let Err(e) = stream.audio_unit.start() {
                let description = format!("{}", e);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            stream.playing = true;
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        let mut stream = self.inner.lock().unwrap();

        if stream.playing {
            if let Err(e) = stream.audio_unit.stop() {
                let description = format!("{}", e);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }

            stream.playing = false;
        }
        Ok(())
    }
}

fn get_io_buffer_frame_size_range(
    audio_unit: &AudioUnit,
) -> Result<SupportedBufferSize, coreaudio::Error> {
    let buffer_size_range: AudioValueRange = audio_unit.get_property(
        kAudioDevicePropertyBufferFrameSizeRange,
        Scope::Global,
        Element::Output,
    )?;

    Ok(SupportedBufferSize::Range {
        min: buffer_size_range.mMinimum as u32,
        max: buffer_size_range.mMaximum as u32,
    })
}

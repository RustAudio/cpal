use super::OSStatus;
use super::Stream;
use super::{
    asbd_from_config, check_os_status, frames_to_duration, host_time_to_stream_instant,
    DuplexCallbackPtr,
};
use crate::duplex::DuplexCallbackInfo;
use crate::host::coreaudio::macos::loopback::LoopbackDevice;
use crate::host::coreaudio::macos::StreamInner;
use crate::traits::DeviceTrait;
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, ChannelCount, Data,
    DefaultStreamConfigError, DeviceId, DeviceIdError, DeviceNameError, InputCallbackInfo,
    OutputCallbackInfo, SampleFormat, SampleRate, StreamConfig, StreamError, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};
use coreaudio::audio_unit::render_callback::{self, data};
use coreaudio::audio_unit::{AudioUnit, Element, Scope};
use objc2_audio_toolbox::{
    kAudioOutputUnitProperty_CurrentDevice, kAudioOutputUnitProperty_EnableIO,
    kAudioUnitProperty_SetRenderCallback, kAudioUnitProperty_StreamFormat, AURenderCallbackStruct,
    AudioUnitRender, AudioUnitRenderActionFlags,
};
use objc2_core_audio::kAudioDevicePropertyDeviceUID;
use objc2_core_audio::kAudioObjectPropertyElementMain;
use objc2_core_audio::{
    kAudioAggregateDeviceClassID, kAudioDevicePropertyAvailableNominalSampleRates,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyBufferFrameSizeRange,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreamFormat, kAudioObjectPropertyClass, kAudioObjectPropertyElementMaster,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput,
    kAudioObjectPropertyScopeOutput, AudioClassID, AudioDeviceID, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectPropertyScope, AudioObjectSetPropertyData,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioValueRange,
};
use objc2_core_foundation::CFString;
use objc2_core_foundation::Type;

pub use super::enumerate::{
    default_input_device, default_output_device, SupportedInputConfigs, SupportedOutputConfigs,
};
use std::fmt;
use std::mem::{self, size_of};
use std::ptr::{null, NonNull};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::invoke_error_callback;
use super::property_listener::AudioObjectPropertyListener;
use coreaudio::audio_unit::macos_helpers::get_device_name;

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
    let mut sample_rate: f64 = 0.0;
    let mut data_size = mem::size_of::<f64>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            audio_device_id,
            NonNull::from(&property_address),
            0,
            null(),
            NonNull::from(&mut data_size),
            NonNull::from(&mut sample_rate).cast(),
        )
    };
    coreaudio::Error::from_os_status(status)?;

    // If the requested sample rate is different to the device sample rate, update the device.
    if sample_rate as u32 != target_sample_rate {
        // Get available sample rate ranges.
        property_address.mSelector = kAudioDevicePropertyAvailableNominalSampleRates;
        let mut data_size = 0u32;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&mut data_size),
            )
        };
        coreaudio::Error::from_os_status(status)?;
        let n_ranges = data_size as usize / mem::size_of::<AudioValueRange>();
        let mut ranges: Vec<AudioValueRange> = Vec::with_capacity(n_ranges);
        let status = unsafe {
            AudioObjectGetPropertyData(
                audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&mut data_size),
                NonNull::new(ranges.as_mut_ptr()).unwrap().cast(),
            )
        };
        coreaudio::Error::from_os_status(status)?;
        unsafe {
            ranges.set_len(n_ranges);
        }

        // Now that we have the available ranges, pick the one matching the desired rate.
        let sample_rate = target_sample_rate;
        if !ranges
            .iter()
            .any(|r| sample_rate as f64 >= r.mMinimum && sample_rate as f64 <= r.mMaximum)
        {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        let (send, recv) = channel::<Result<f64, coreaudio::Error>>();
        let sample_rate_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyNominalSampleRate,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        // Send sample rate updates back on a channel.
        let sample_rate_handler = move || {
            let mut rate: f64 = 0.0;
            let mut data_size = mem::size_of::<f64>() as u32;

            let result = unsafe {
                AudioObjectGetPropertyData(
                    audio_device_id,
                    NonNull::from(&sample_rate_address),
                    0,
                    null(),
                    NonNull::from(&mut data_size),
                    NonNull::from(&mut rate).cast(),
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
        // Set the nominal sample rate using a single f64 as required by CoreAudio.
        let rate = sample_rate as f64;
        let data_size = mem::size_of::<f64>() as u32;
        let status = unsafe {
            AudioObjectSetPropertyData(
                audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                data_size,
                NonNull::from(&rate).cast(),
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
                    if reported_sample_rate == target_sample_rate as f64 {
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

fn audio_unit_from_device(device: &Device, input: bool) -> Result<AudioUnit, coreaudio::Error> {
    let output_type = if !input && is_default_output_device(device) {
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

    // Device selection is a device-level property: always use Scope::Global + Element::Output
    audio_unit.set_property(
        kAudioOutputUnitProperty_CurrentDevice,
        Scope::Global,
        Element::Output,
        Some(&device.audio_device_id),
    )?;

    Ok(audio_unit)
}

fn get_io_buffer_frame_size_range(
    audio_unit: &AudioUnit,
) -> Result<SupportedBufferSize, coreaudio::Error> {
    // Device-level property: always use Scope::Global + Element::Output
    // regardless of whether this audio unit is configured for input or output
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

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<crate::DeviceDescription, DeviceNameError> {
        Device::description(self)
    }

    fn id(&self) -> Result<DeviceId, DeviceIdError> {
        Device::id(self)
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

    fn build_duplex_stream_raw<D, E>(
        &self,
        config: &crate::duplex::DuplexStreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Device::build_duplex_stream_raw(self, config, sample_format, data_callback, error_callback)
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct Device {
    pub(crate) audio_device_id: AudioDeviceID,
}

fn is_default_input_device(device: &Device) -> bool {
    default_input_device().is_some_and(|d| d.audio_device_id == device.audio_device_id)
}

fn is_default_output_device(device: &Device) -> bool {
    default_output_device().is_some_and(|d| d.audio_device_id == device.audio_device_id)
}

impl Device {
    /// Construct a new device given its ID.
    /// Useful for constructing hidden devices.
    pub fn new(audio_device_id: AudioDeviceID) -> Self {
        Self { audio_device_id }
    }

    /// Checks if this device is an aggregate device.
    ///
    /// Aggregate devices combine multiple physical devices into a single logical device.
    fn is_aggregate_device(&self) -> bool {
        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioObjectPropertyClass,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut class_id: AudioClassID = 0;
        let data_size = size_of::<AudioClassID>() as u32;

        // SAFETY: AudioObjectGetPropertyData is documented to write an AudioClassID
        // for kAudioObjectPropertyClass. We check the status before using the value.
        let status = unsafe {
            AudioObjectGetPropertyData(
                self.audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&data_size),
                NonNull::from(&mut class_id).cast(),
            )
        };

        // If successful, check if it's an aggregate device
        status == 0 && class_id == kAudioAggregateDeviceClassID
    }

    fn description(&self) -> Result<crate::DeviceDescription, DeviceNameError> {
        let name = get_device_name(self.audio_device_id).map_err(|err| {
            DeviceNameError::BackendSpecific {
                err: BackendSpecificError {
                    description: err.to_string(),
                },
            }
        })?;

        let input_configs = self
            .supported_input_configs()
            .map(|configs| configs.count() as ChannelCount)
            .ok();
        let output_configs = self
            .supported_output_configs()
            .map(|configs| configs.count() as ChannelCount)
            .ok();

        let direction =
            crate::device_description::direction_from_counts(input_configs, output_configs);

        let mut builder = crate::DeviceDescriptionBuilder::new(name).direction(direction);

        // Check if this is an aggregate device
        if self.is_aggregate_device() {
            builder = builder.interface_type(crate::InterfaceType::Aggregate);
        }

        Ok(builder.build())
    }

    fn id(&self) -> Result<DeviceId, DeviceIdError> {
        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyDeviceUID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        // CFString is copied from the audio object, use wrap_under_create_rule
        let mut uid: *mut CFString = std::ptr::null_mut();
        let mut data_size = size_of::<*mut CFString>() as u32;

        // SAFETY: AudioObjectGetPropertyData is documented to write a CFString pointer
        // for kAudioDevicePropertyDeviceUID. We check the status code before use.
        let status = unsafe {
            AudioObjectGetPropertyData(
                self.audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&mut data_size),
                NonNull::from(&mut uid).cast(),
            )
        };
        check_os_status(status)?;

        // SAFETY: Status was successful, meaning the API call succeeded.
        // We now check if the returned uid is non-null before use.
        if !uid.is_null() {
            let uid_string = unsafe { CFString::wrap_under_create_rule(uid).to_string() };
            Ok(DeviceId(crate::platform::HostId::CoreAudio, uid_string))
        } else {
            Err(DeviceIdError::BackendSpecific {
                err: BackendSpecificError {
                    description: "Device UID is null".to_string(),
                },
            })
        }
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
            let mut data_size = 0u32;
            let status = AudioObjectGetPropertyDataSize(
                self.audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&mut data_size),
            );
            check_os_status(status)?;

            let mut audio_buffer_list: Vec<u8> = vec![];
            audio_buffer_list.reserve_exact(data_size as usize);
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&mut data_size),
                NonNull::new(audio_buffer_list.as_mut_ptr()).unwrap().cast(),
            );
            check_os_status(status)?;

            let audio_buffer_list = audio_buffer_list.as_mut_ptr() as *mut AudioBufferList;

            // Read the number of buffers without assuming alignment (avoid UB).
            let nb_ptr = core::ptr::addr_of!((*audio_buffer_list).mNumberBuffers);
            let n_buffers = core::ptr::read_unaligned(nb_ptr) as usize;
            // If there are no buffers, skip.
            if n_buffers == 0 {
                return Ok(vec![].into_iter());
            }

            // Count the number of channels as the sum of all channels in all output buffers.
            let first_buf_ptr =
                core::ptr::addr_of!((*audio_buffer_list).mBuffers) as *const AudioBuffer;
            let mut n_channels = 0usize;
            for i in 0..n_buffers {
                let buf_ptr = first_buf_ptr.add(i);
                // Read potentially unaligned
                let buf: AudioBuffer = core::ptr::read_unaligned(buf_ptr);
                n_channels += buf.mNumberChannels as usize;
            }

            // TODO: macOS should support U8, I16, I32, F32 and F64. This should allow for using
            // I16 but just use F32 for now as it's the default anyway.
            let sample_format = SampleFormat::F32;

            // Get available sample rate ranges.
            // The property "kAudioDevicePropertyAvailableNominalSampleRates" returns a list of pairs of
            // minimum and maximum sample rates but most of the devices returns pairs of same values though the underlying mechanism is unclear.
            // This may cause issues when, for example, sorting the configs by the sample rates.
            // We follows the implementation of RtAudio, which returns single element of config
            // when all the pairs have the same values and returns multiple elements otherwise.
            // See https://github.com/thestk/rtaudio/blob/master/RtAudio.cpp#L1369C1-L1375C39

            property_address.mSelector = kAudioDevicePropertyAvailableNominalSampleRates;
            let mut data_size = 0u32;
            let status = AudioObjectGetPropertyDataSize(
                self.audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&mut data_size),
            );
            check_os_status(status)?;

            let n_ranges = data_size as usize / mem::size_of::<AudioValueRange>();
            let mut ranges: Vec<AudioValueRange> = Vec::with_capacity(n_ranges);
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&mut data_size),
                NonNull::new(ranges.as_mut_ptr()).unwrap().cast(),
            );
            check_os_status(status)?;

            ranges.set_len(n_ranges);

            #[allow(non_upper_case_globals)]
            let input = match scope {
                kAudioObjectPropertyScopeInput => Ok(true),
                kAudioObjectPropertyScopeOutput => Ok(false),
                _ => Err(BackendSpecificError {
                    description: format!("unexpected scope (neither input nor output): {scope:?}"),
                }),
            }?;
            let audio_unit = audio_unit_from_device(self, input)?;
            let buffer_size = get_io_buffer_frame_size_range(&audio_unit)?;

            // Collect the supported formats for the device.

            let contains_different_sample_rates = ranges.iter().any(|r| r.mMinimum != r.mMaximum);
            if ranges.is_empty() {
                Ok(vec![].into_iter())
            } else if contains_different_sample_rates {
                let res = ranges.iter().map(|range| SupportedStreamConfigRange {
                    channels: n_channels as ChannelCount,
                    min_sample_rate: range.mMinimum as u32,
                    max_sample_rate: range.mMaximum as u32,
                    buffer_size,
                    sample_format,
                });
                Ok(res.collect::<Vec<_>>().into_iter())
            } else {
                let fmt = SupportedStreamConfigRange {
                    channels: n_channels as ChannelCount,
                    min_sample_rate: ranges
                        .iter()
                        .map(|v| v.mMinimum as u32)
                        .min()
                        .expect("the list must not be empty"),
                    max_sample_rate: ranges
                        .iter()
                        .map(|v| v.mMaximum as u32)
                        .max()
                        .expect("the list must not be empty"),
                    buffer_size,
                    sample_format,
                };

                Ok(vec![fmt].into_iter())
            }
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
                    let description = format!("{err}");
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
            let mut asbd: AudioStreamBasicDescription = mem::zeroed();
            let mut data_size = mem::size_of::<AudioStreamBasicDescription>() as u32;
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                NonNull::from(&property_address),
                0,
                null(),
                NonNull::from(&mut data_size),
                NonNull::from(&mut asbd).cast(),
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

            #[allow(non_upper_case_globals)]
            let input = match scope {
                kAudioObjectPropertyScopeInput => Ok(true),
                kAudioObjectPropertyScopeOutput => Ok(false),
                _ => Err(BackendSpecificError {
                    description: format!("unexpected scope (neither input nor output): {scope:?}"),
                }),
            }?;
            let audio_unit = audio_unit_from_device(self, input)?;
            let buffer_size = get_io_buffer_frame_size_range(&audio_unit)?;

            let config = SupportedStreamConfig {
                sample_rate: asbd.mSampleRate as _,
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

    /// Check if this device supports input (recording).
    fn supports_input(&self) -> bool {
        // Check if the device has input channels by trying to get its input configuration
        self.supported_input_configs()
            .map(|mut configs| configs.next().is_some())
            .unwrap_or(false)
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

        let mut loopback_aggregate: Option<LoopbackDevice> = None;
        let mut audio_unit = if self.supports_input() {
            audio_unit_from_device(self, true)?
        } else {
            loopback_aggregate.replace(LoopbackDevice::from_device(self)?);
            audio_unit_from_device(&loopback_aggregate.as_ref().unwrap().aggregate_device, true)?
        };

        // Configure stream format and buffer size for predictable callback behavior.
        configure_stream_format_and_buffer(&mut audio_unit, config, sample_format, scope, element)?;

        let error_callback = Arc::new(Mutex::new(error_callback));
        let error_callback_disconnect = error_callback.clone();

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let (bytes_per_channel, sample_rate, device_buffer_frames) =
            setup_callback_vars(&audio_unit, config, sample_format);

        type Args = render_callback::Args<data::Raw>;
        audio_unit.set_input_callback(move |args: Args| unsafe {
            // SAFETY: We configure the stream format as interleaved (via asbd_from_config which
            // does not set kAudioFormatFlagIsNonInterleaved). Interleaved format always has
            // exactly one buffer containing all channels, so mBuffers[0] is always valid.
            let AudioBuffer {
                mNumberChannels: channels,
                mDataByteSize: data_byte_size,
                mData: data,
            } = (*args.data.data).mBuffers[0];

            let data = data as *mut ();
            let len = data_byte_size as usize / bytes_per_channel;
            let data = Data::from_parts(data, len, sample_format);

            let callback = match host_time_to_stream_instant(args.time_stamp.mHostTime) {
                Err(err) => {
                    invoke_error_callback(&error_callback, err.into());
                    return Err(());
                }
                Ok(cb) => cb,
            };
            let buffer_frames = len / channels as usize;
            // Use device buffer size for latency calculation if available
            let latency_frames = device_buffer_frames.unwrap_or(
                // Fallback to callback buffer size if device buffer size is unknown
                // (may overestimate latency for BufferSize::Default)
                buffer_frames,
            );
            let delay = frames_to_duration(latency_frames, sample_rate);
            let capture = callback
                .sub(delay)
                .expect("`capture` occurs before origin of alsa `StreamInstant`");
            let timestamp = crate::InputStreamTimestamp { callback, capture };

            let info = InputCallbackInfo { timestamp };
            data_callback(&data, &info);
            Ok(())
        })?;

        // Create error callback for stream - either dummy or real based on device type
        let error_callback_for_stream: super::ErrorCallback = if is_default_input_device(self) {
            Box::new(|_: StreamError| {})
        } else {
            let error_callback_clone = error_callback_disconnect.clone();
            Box::new(move |err: StreamError| {
                invoke_error_callback(&error_callback_clone, err);
            })
        };

        let stream = Stream::new(
            StreamInner {
                playing: true,
                audio_unit,
                device_id: self.audio_device_id,
                _loopback_device: loopback_aggregate,
                duplex_callback_ptr: None,
            },
            error_callback_for_stream,
        )?;

        stream
            .inner
            .lock()
            .map_err(|_| BuildStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "A cpal stream operation panicked while holding the lock - this is a bug, please report it".to_string(),
                },
            })?
            .audio_unit
            .start()?;

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

        // Configure device buffer (see comprehensive documentation in input stream above)
        configure_stream_format_and_buffer(&mut audio_unit, config, sample_format, scope, element)?;

        let error_callback = Arc::new(Mutex::new(error_callback));
        let error_callback_disconnect = error_callback.clone();

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let (bytes_per_channel, sample_rate, device_buffer_frames) =
            setup_callback_vars(&audio_unit, config, sample_format);

        type Args = render_callback::Args<data::Raw>;
        audio_unit.set_render_callback(move |args: Args| unsafe {
            // SAFETY: We configure the stream format as interleaved (via asbd_from_config which
            // does not set kAudioFormatFlagIsNonInterleaved). Interleaved format always has
            // exactly one buffer containing all channels, so mBuffers[0] is always valid.
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
                    invoke_error_callback(&error_callback, err.into());
                    return Err(());
                }
                Ok(cb) => cb,
            };
            let buffer_frames = len / channels as usize;
            // Use device buffer size for latency calculation if available
            let latency_frames = device_buffer_frames.unwrap_or(
                // Fallback to callback buffer size if device buffer size is unknown
                // (may overestimate latency for BufferSize::Default)
                buffer_frames,
            );
            let delay = frames_to_duration(latency_frames, sample_rate);
            let playback = callback
                .add(delay)
                .expect("`playback` occurs beyond representation supported by `StreamInstant`");
            let timestamp = crate::OutputStreamTimestamp { callback, playback };

            let info = OutputCallbackInfo { timestamp };
            data_callback(&mut data, &info);
            Ok(())
        })?;

        // Create error callback for stream - either dummy or real based on device type
        let error_callback_for_stream: super::ErrorCallback = if is_default_output_device(self) {
            Box::new(|_: StreamError| {})
        } else {
            let error_callback_clone = error_callback_disconnect.clone();
            Box::new(move |err: StreamError| {
                invoke_error_callback(&error_callback_clone, err);
            })
        };

        let stream = Stream::new(
            StreamInner {
                playing: true,
                audio_unit,
                device_id: self.audio_device_id,
                _loopback_device: None,
                duplex_callback_ptr: None,
            },
            error_callback_for_stream,
        )?;

        stream
            .inner
            .lock()
            .map_err(|_| BuildStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "A cpal stream operation panicked while holding the lock - this is a bug, please report it".to_string(),
                },
            })?
            .audio_unit
            .start()?;

        Ok(stream)
    }

    /// Build a duplex stream with synchronized input and output.
    ///
    /// This creates a single HAL AudioUnit with both input and output enabled,
    /// ensuring they share the same hardware clock.
    fn build_duplex_stream_raw<D, E>(
        &self,
        config: &crate::duplex::DuplexStreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // Validate that device supports duplex
        if !self.supports_input() || !self.supports_output() {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        // Potentially change the device sample rate to match the config.
        set_sample_rate(self.audio_device_id, config.sample_rate)?;

        // Create HAL AudioUnit - always use HalOutput for duplex
        let mut audio_unit = AudioUnit::new(coreaudio::audio_unit::IOType::HalOutput)?;

        // Enable BOTH input and output on the AudioUnit
        let enable: u32 = 1;

        // Enable input on Element 1
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Input,
            Element::Input,
            Some(&enable),
        )?;

        // Enable output on Element 0 (usually enabled by default, but be explicit)
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Output,
            Element::Output,
            Some(&enable),
        )?;

        // Set device for the unit (applies to both input and output)
        audio_unit.set_property(
            kAudioOutputUnitProperty_CurrentDevice,
            Scope::Global,
            Element::Output,
            Some(&self.audio_device_id),
        )?;

        // Create StreamConfig for input side
        let input_stream_config = StreamConfig {
            channels: config.input_channels as ChannelCount,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };

        // Create StreamConfig for output side
        let output_stream_config = StreamConfig {
            channels: config.output_channels as ChannelCount,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };

        // Configure input format (Scope::Output on Element::Input)
        let input_asbd = asbd_from_config(&input_stream_config, sample_format);
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Output,
            Element::Input,
            Some(&input_asbd),
        )?;

        // Configure output format (Scope::Input on Element::Output)
        let output_asbd = asbd_from_config(&output_stream_config, sample_format);
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&output_asbd),
        )?;

        // Configure buffer size if requested
        if let BufferSize::Fixed(buffer_size) = &config.buffer_size {
            audio_unit.set_property(
                kAudioDevicePropertyBufferFrameSize,
                Scope::Global,
                Element::Output,
                Some(buffer_size),
            )?;
        }

        // Get actual buffer size for pre-allocating input buffer
        let buffer_size: u32 = audio_unit
            .get_property(
                kAudioDevicePropertyBufferFrameSize,
                Scope::Global,
                Element::Output,
            )
            .unwrap_or(512);

        // Get callback vars for latency calculation (matching input/output pattern)
        let sample_rate = config.sample_rate;
        let device_buffer_frames = get_device_buffer_frame_size(&audio_unit).ok();

        // Get the raw AudioUnit pointer for use in the callback
        let raw_audio_unit = *audio_unit.as_ref();

        // Configuration for callback
        let input_channels = config.input_channels as usize;
        let sample_bytes = sample_format.sample_size();

        // Pre-allocate input buffer for the configured buffer size (in bytes)
        let input_buffer_samples = buffer_size as usize * input_channels;
        let input_buffer_bytes = input_buffer_samples * sample_bytes;
        let mut input_buffer: Vec<u8> = vec![0u8; input_buffer_bytes];

        // Wrap error callback in Arc<Mutex> for sharing between callback and disconnect handler
        let error_callback = Arc::new(Mutex::new(error_callback));
        let error_callback_for_callback = error_callback.clone();

        // Move data callback into closure
        let mut data_callback = data_callback;

        // Create the duplex callback closure
        // This closure owns all captured state - no Mutex needed for data_callback or input_buffer
        let duplex_proc: Box<DuplexProcFn> = Box::new(
            move |io_action_flags: NonNull<AudioUnitRenderActionFlags>,
                  in_time_stamp: NonNull<AudioTimeStamp>,
                  _in_bus_number: u32,
                  in_number_frames: u32,
                  io_data: *mut AudioBufferList|
                  -> i32 {
                let num_frames = in_number_frames as usize;
                let input_samples = num_frames * input_channels;
                let input_bytes = input_samples * sample_bytes;

                // SAFETY: in_time_stamp is valid per CoreAudio contract
                let timestamp = unsafe { in_time_stamp.as_ref() };

                // Create StreamInstant for callback_instant
                let callback_instant = match host_time_to_stream_instant(timestamp.mHostTime) {
                    Err(err) => {
                        invoke_error_callback(&error_callback_for_callback, err.into());
                        return 0;
                    }
                    Ok(cb) => cb,
                };

                // Calculate latency-adjusted timestamps (matching input/output pattern)
                let buffer_frames = num_frames;
                // Use device buffer size for latency calculation if available
                let latency_frames = device_buffer_frames.unwrap_or(
                    // Fallback to callback buffer size if device buffer size is unknown
                    buffer_frames,
                );
                let delay = frames_to_duration(latency_frames, sample_rate);

                // Capture time: when input was actually captured (in the past)
                let capture = callback_instant
                    .sub(delay)
                    .expect("`capture` occurs before origin of `StreamInstant`");

                // Playback time: when output will actually play (in the future)
                let playback = callback_instant
                    .add(delay)
                    .expect("`playback` occurs beyond representation supported by `StreamInstant`");

                // Pull input from Element 1 using AudioUnitRender
                // We use the pre-allocated input_buffer
                unsafe {
                    // Set up AudioBufferList pointing to our input buffer
                    let mut input_buffer_list = AudioBufferList {
                        mNumberBuffers: 1,
                        mBuffers: [AudioBuffer {
                            mNumberChannels: input_channels as u32,
                            mDataByteSize: input_bytes as u32,
                            mData: input_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                        }],
                    };

                    let status = AudioUnitRender(
                        raw_audio_unit,
                        io_action_flags.as_ptr(),
                        in_time_stamp,
                        1, // Element 1 = input
                        in_number_frames,
                        NonNull::new_unchecked(&mut input_buffer_list),
                    );

                    if status != 0 {
                        // Report error but continue with silence for graceful degradation
                        invoke_error_callback(
                            &error_callback_for_callback,
                            StreamError::BackendSpecific {
                                err: BackendSpecificError {
                                    description: format!(
                                        "AudioUnitRender failed for input: OSStatus {}",
                                        status
                                    ),
                                },
                            },
                        );
                        input_buffer[..input_bytes].fill(0);
                    }
                }

                // Get output buffer from CoreAudio
                if io_data.is_null() {
                    return 0;
                }

                // Create Data wrappers for input and output
                let input_data = unsafe {
                    Data::from_parts(
                        input_buffer.as_mut_ptr() as *mut (),
                        input_samples,
                        sample_format,
                    )
                };

                let mut output_data = unsafe {
                    let buffer_list = &mut *io_data;
                    if buffer_list.mNumberBuffers == 0 {
                        return 0;
                    }
                    let buffer = &mut buffer_list.mBuffers[0];
                    if buffer.mData.is_null() {
                        return 0;
                    }
                    let output_samples = buffer.mDataByteSize as usize / sample_bytes;
                    Data::from_parts(buffer.mData as *mut (), output_samples, sample_format)
                };

                // Create callback info with latency-adjusted times
                let input_timestamp = crate::InputStreamTimestamp {
                    callback: callback_instant,
                    capture,
                };
                let output_timestamp = crate::OutputStreamTimestamp {
                    callback: callback_instant,
                    playback,
                };
                let callback_info = DuplexCallbackInfo::new(input_timestamp, output_timestamp);

                // Call user callback with input and output Data
                data_callback(&input_data, &mut output_data, &callback_info);

                0 // noErr
            },
        );

        // Box the wrapper and get raw pointer for CoreAudio
        let wrapper = Box::new(DuplexProcWrapper {
            callback: duplex_proc,
        });
        let wrapper_ptr = Box::into_raw(wrapper);

        // Set up the render callback
        let render_callback = AURenderCallbackStruct {
            inputProc: Some(duplex_input_proc),
            inputProcRefCon: wrapper_ptr as *mut std::ffi::c_void,
        };

        audio_unit.set_property(
            kAudioUnitProperty_SetRenderCallback,
            Scope::Global,
            Element::Output,
            Some(&render_callback),
        )?;

        // Create the stream inner, storing the callback pointer for cleanup
        let inner = StreamInner {
            playing: true,
            audio_unit,
            device_id: self.audio_device_id,
            _loopback_device: None,
            duplex_callback_ptr: Some(DuplexCallbackPtr(wrapper_ptr)),
        };

        // Create error callback for stream - either dummy or real based on device type
        // For duplex, check both input and output default device status
        let error_callback_for_stream: super::ErrorCallback =
            if is_default_input_device(self) || is_default_output_device(self) {
                Box::new(|_: StreamError| {})
            } else {
                let error_callback_clone = error_callback.clone();
                Box::new(move |err: StreamError| {
                    invoke_error_callback(&error_callback_clone, err);
                })
            };

        // Create the duplex stream
        let stream = Stream::new(inner, error_callback_for_stream)?;

        // Start the audio unit
        stream
            .inner
            .lock()
            .map_err(|_| BuildStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "Failed to lock duplex stream".to_string(),
                },
            })?
            .audio_unit
            .start()?;

        Ok(stream)
    }
}

/// Configure stream format and buffer size for CoreAudio stream.
///
/// This handles the common setup tasks for both input and output streams:
/// - Sets the stream format (ASBD)
/// - Configures buffer size for Fixed buffer size requests
fn configure_stream_format_and_buffer(
    audio_unit: &mut AudioUnit,
    config: &StreamConfig,
    sample_format: SampleFormat,
    scope: Scope,
    element: Element,
) -> Result<(), BuildStreamError> {
    // Set the stream format using stream-specific scope/element
    // - Input streams: scope=Output, element=Input (configuring output format of input element)
    // - Output streams: scope=Input, element=Output (configuring input format of output element)
    let asbd = asbd_from_config(config, sample_format);
    audio_unit.set_property(kAudioUnitProperty_StreamFormat, scope, element, Some(&asbd))?;

    // Configure device buffer size if requested
    if let BufferSize::Fixed(buffer_size) = config.buffer_size {
        // IMPORTANT: Buffer frame size is a DEVICE-LEVEL property, not stream-specific.
        // Unlike stream format above, we ALWAYS use Scope::Global + Element::Output
        // for device properties, regardless of whether this is an input or output stream.
        // This is consistent with other device properties like:
        // - kAudioOutputUnitProperty_CurrentDevice
        // - kAudioDevicePropertyBufferFrameSizeRange
        // The Element::Output here doesn't mean "output stream only" - it's the
        // canonical element used for device-wide properties in Core Audio.
        audio_unit.set_property(
            kAudioDevicePropertyBufferFrameSize,
            Scope::Global,
            Element::Output,
            Some(&buffer_size),
        )?;
    }

    Ok(())
}

/// Setup common callback variables and query device buffer size.
///
/// Returns (bytes_per_channel, sample_rate, device_buffer_frames)
fn setup_callback_vars(
    audio_unit: &AudioUnit,
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> (usize, crate::SampleRate, Option<usize>) {
    let bytes_per_channel = sample_format.sample_size();
    let sample_rate = config.sample_rate;

    // Query device buffer size for latency calculation
    let device_buffer_frames = get_device_buffer_frame_size(audio_unit).ok();

    (bytes_per_channel, sample_rate, device_buffer_frames)
}

/// Query the current device buffer frame size from CoreAudio.
///
/// Buffer frame size is a device-level property that always uses Scope::Global + Element::Output,
/// regardless of whether the audio unit is configured for input or output streams.
fn get_device_buffer_frame_size(audio_unit: &AudioUnit) -> Result<usize, coreaudio::Error> {
    // Device-level property: always use Scope::Global + Element::Output
    // This is consistent with how we set the buffer size and query the buffer size range
    let frames: u32 = audio_unit.get_property(
        kAudioDevicePropertyBufferFrameSize,
        Scope::Global,
        Element::Output,
    )?;
    Ok(frames as usize)
}

// ============================================================================
// Duplex callback infrastructure
// ============================================================================

use std::ffi::c_void;

/// Type alias for the duplex callback closure.
///
/// This is the raw callback signature that CoreAudio expects. The closure
/// receives the same parameters as a C callback and returns an OSStatus.
type DuplexProcFn = dyn FnMut(
    NonNull<AudioUnitRenderActionFlags>,
    NonNull<AudioTimeStamp>,
    u32, // bus_number
    u32, // num_frames
    *mut AudioBufferList,
) -> i32;

/// Wrapper for the boxed duplex callback closure.
///
/// This struct is allocated on the heap and its pointer is passed to CoreAudio
/// as the refcon. The extern "C" callback function casts the refcon back to
/// this type and calls the closure.
pub(crate) struct DuplexProcWrapper {
    callback: Box<DuplexProcFn>,
}

// SAFETY: DuplexProcWrapper is Send because:
// 1. The boxed closure captures only Send types (the DuplexCallback trait requires Send)
// 2. The raw pointer stored in StreamInner is accessed:
//    - By CoreAudio's audio thread via `duplex_input_proc` (as the refcon)
//    - During Drop, after stopping the audio unit (callback no longer running)
//    These never overlap: Drop stops the audio unit before reclaiming the pointer.
// 3. CoreAudio guarantees single-threaded callback invocation
unsafe impl Send for DuplexProcWrapper {}

/// CoreAudio render callback for duplex audio.
///
/// This is a thin wrapper that casts the refcon back to our DuplexProcWrapper
/// and calls the inner closure. The closure owns all the callback state via
/// move semantics, so no Mutex is needed.
///
/// Note: `extern "C-unwind"` is required here because `AURenderCallbackStruct`
/// from coreaudio-sys types the `inputProc` field as `extern "C-unwind"`.
/// Ideally this would be `extern "C"` so that a panic aborts rather than
/// unwinding through CoreAudio's C frames (this callback runs on CoreAudio's
/// audio thread with no Rust frames above to catch an unwind). Per RFC 2945
/// (https://github.com/rust-lang/rust/issues/115285), `extern "C"` aborts on
/// panic, which would be the correct behavior here.
extern "C-unwind" fn duplex_input_proc(
    in_ref_con: NonNull<c_void>,
    io_action_flags: NonNull<AudioUnitRenderActionFlags>,
    in_time_stamp: NonNull<AudioTimeStamp>,
    in_bus_number: u32,
    in_number_frames: u32,
    io_data: *mut AudioBufferList,
) -> i32 {
    // SAFETY: `in_ref_con` points to a heap-allocated `DuplexProcWrapper` created
    // via `Box::into_raw` in `build_duplex_stream`, and remains valid for the
    // lifetime of the audio unit (reclaimed in `StreamInner::drop`). The `as_mut()` call
    // produces an exclusive `&mut` reference, which is sound because CoreAudio
    // guarantees single-threaded callback invocation — this function is never
    // called concurrently, so only one `&mut` to the wrapper exists at a time.
    let wrapper = unsafe { in_ref_con.cast::<DuplexProcWrapper>().as_mut() };
    (wrapper.callback)(
        io_action_flags,
        in_time_stamp,
        in_bus_number,
        in_number_frames,
        io_data,
    )
}

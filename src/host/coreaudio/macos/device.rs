use std::{
    fmt,
    ptr::{NonNull, null},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use coreaudio::audio_unit::{
    AudioUnit, Element, SampleFormat as CoreAudioSampleFormat, Scope,
    macos_helpers::{
        audio_unit_from_device_id_uninitialized, get_device_name,
        get_supported_physical_stream_formats,
    },
    render_callback::{self, data},
};
use objc2_audio_toolbox::{
    kAudioOutputUnitProperty_CurrentDevice, kAudioUnitProperty_StreamFormat,
};
use objc2_core_audio::{
    AudioClassID, AudioDeviceID, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectPropertyAddress, AudioObjectPropertyScope, kAudioAggregateDeviceClassID,
    kAudioDevicePropertyAvailableNominalSampleRates, kAudioDevicePropertyBufferFrameSize,
    kAudioDevicePropertyBufferFrameSizeRange, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyLatency, kAudioDevicePropertySafetyOffset,
    kAudioDevicePropertyStreamConfiguration, kAudioDevicePropertyStreamFormat,
    kAudioDevicePropertyTransportType, kAudioDeviceTransportTypeAVB,
    kAudioDeviceTransportTypeAggregate, kAudioDeviceTransportTypeAirPlay,
    kAudioDeviceTransportTypeBluetooth, kAudioDeviceTransportTypeBluetoothLE,
    kAudioDeviceTransportTypeBuiltIn, kAudioDeviceTransportTypeDisplayPort,
    kAudioDeviceTransportTypeFireWire, kAudioDeviceTransportTypeHDMI, kAudioDeviceTransportTypePCI,
    kAudioDeviceTransportTypeThunderbolt, kAudioDeviceTransportTypeUSB,
    kAudioDeviceTransportTypeVirtual, kAudioObjectPropertyClass, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput,
    kAudioObjectPropertyScopeOutput,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioValueRange,
};
use objc2_core_foundation::{CFRetained, CFString};

pub use super::enumerate::{SupportedInputConfigs, SupportedOutputConfigs};
use super::{
    DefaultOutputMonitor, DisconnectManager, Monitor, Stream, asbd_from_config, check_os_status,
    format::{set_physical_format, set_sample_rate},
    host_time_to_stream_instant,
    property::{get_property, get_property_array},
};
use crate::{
    BufferSize, CallbackInfo, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder,
    DeviceId, Error, ErrorKind, FrameCount, InterfaceType, ResultExt, SampleFormat, SampleRate,
    StreamConfig, StreamInstant, StreamTimestamp, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange,
    host::{
        ErrorCallbackArc,
        coreaudio::macos::{StreamInner, loopback::LoopbackDevice},
        equilibrium::fill_equilibrium,
        frames_to_duration, try_emit_error,
    },
    traits::DeviceTrait,
};

#[derive(Clone, Copy)]
enum AudioUnitMode {
    /// HAL Output AudioUnit with input enabled, pinned to a specific device.
    Input,
    /// HAL Output AudioUnit for output, pinned to a specific device.
    Output,
    /// DefaultOutput AudioUnit; follows the system default output device automatically.
    DefaultOutput,
}

fn audio_unit_from_device(
    device: &Device,
    mode: AudioUnitMode,
) -> Result<AudioUnit, coreaudio::Error> {
    match mode {
        AudioUnitMode::DefaultOutput => {
            AudioUnit::new_uninitialized(coreaudio::audio_unit::IOType::DefaultOutput)
        }
        AudioUnitMode::Input => {
            audio_unit_from_device_id_uninitialized(device.audio_device_id, true)
        }
        AudioUnitMode::Output => {
            // Do not use audio_unit_from_device_id_uninitialized here: that function compares the
            // device ID against the live system default and silently switches to DefaultOutput
            // mode if they match. We explicitly pin HalOutput unit here.
            let mut audio_unit =
                AudioUnit::new_uninitialized(coreaudio::audio_unit::IOType::HalOutput)?;
            // Device selection is a device-level property:
            // always use Scope::Global + Element::Output
            audio_unit.set_property(
                kAudioOutputUnitProperty_CurrentDevice,
                Scope::Global,
                Element::Output,
                Some(&device.audio_device_id),
            )?;
            Ok(audio_unit)
        }
    }
}

fn get_io_buffer_frame_size_range(device_id: AudioDeviceID) -> Result<SupportedBufferSize, Error> {
    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyBufferFrameSizeRange,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    // SAFETY: kAudioDevicePropertyBufferFrameSizeRange is documented to return an AudioValueRange.
    let range: AudioValueRange = unsafe { get_property(device_id, property_address) }?;
    Ok(SupportedBufferSize::Range {
        min: range.mMinimum as u32,
        max: range.mMaximum as u32,
    })
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Device::description(self)
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Device::id(self)
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Device::supported_input_configs(self)
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        Device::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Device::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        Device::default_output_config(self)
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
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
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
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
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

#[derive(Clone)]
pub struct Device {
    pub(crate) audio_device_id: AudioDeviceID,
    pub(crate) is_default_output: bool,
}

impl Device {
    /// Construct a new device given its ID.
    /// Useful for constructing hidden devices.
    pub fn new(audio_device_id: AudioDeviceID) -> Self {
        Self {
            audio_device_id,
            is_default_output: false,
        }
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

        // SAFETY: kAudioObjectPropertyClass is documented to return an AudioClassID.
        unsafe { get_property::<AudioClassID>(self.audio_device_id, property_address) }
            .is_ok_and(|class_id| class_id == kAudioAggregateDeviceClassID)
    }

    /// `None` when the property is unavailable or names a transport with no
    /// `InterfaceType` counterpart, so the field is left unset rather than wrong.
    fn transport_interface_type(&self) -> Option<InterfaceType> {
        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyTransportType,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        // SAFETY: kAudioDevicePropertyTransportType is documented to return a UInt32.
        let Ok(transport) =
            (unsafe { get_property::<u32>(self.audio_device_id, property_address) })
        else {
            return None;
        };

        #[allow(non_upper_case_globals)]
        match transport {
            kAudioDeviceTransportTypeBuiltIn => Some(InterfaceType::BuiltIn),
            kAudioDeviceTransportTypeUSB => Some(InterfaceType::Usb),
            kAudioDeviceTransportTypeBluetooth | kAudioDeviceTransportTypeBluetoothLE => {
                Some(InterfaceType::Bluetooth)
            }
            kAudioDeviceTransportTypeVirtual => Some(InterfaceType::Virtual),
            kAudioDeviceTransportTypeAggregate => Some(InterfaceType::Aggregate),
            kAudioDeviceTransportTypeThunderbolt => Some(InterfaceType::Thunderbolt),
            kAudioDeviceTransportTypeHDMI => Some(InterfaceType::Hdmi),
            kAudioDeviceTransportTypeDisplayPort => Some(InterfaceType::DisplayPort),
            kAudioDeviceTransportTypeFireWire => Some(InterfaceType::FireWire),
            kAudioDeviceTransportTypePCI => Some(InterfaceType::Pci),
            kAudioDeviceTransportTypeAirPlay | kAudioDeviceTransportTypeAVB => {
                Some(InterfaceType::Network)
            }
            _ => None,
        }
    }

    fn description(&self) -> Result<crate::DeviceDescription, Error> {
        let name = get_device_name(self.audio_device_id).context("Failed to get device name")?;

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

        let mut builder = DeviceDescriptionBuilder::new(name).direction(direction);

        // TransportType also reports "grup" for aggregates; the class check
        // remains for devices that do not expose the property.
        if let Some(interface_type) = self.transport_interface_type() {
            builder = builder.interface_type(interface_type);
        } else if self.is_aggregate_device() {
            builder = builder.interface_type(InterfaceType::Aggregate);
        }

        Ok(builder.build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyDeviceUID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        // CFString is returned under the create rule, so take ownership of the +1 reference.
        // SAFETY: kAudioDevicePropertyDeviceUID is documented to return a CFString pointer.
        let uid: *mut CFString = unsafe { get_property(self.audio_device_id, property_address) }?;

        // SAFETY: Status was successful, meaning the API call succeeded.
        // We now check if the returned uid is non-null before use.
        if !uid.is_null() {
            let uid_string =
                unsafe { CFRetained::from_raw(NonNull::new(uid).unwrap()).to_string() };
            Ok(DeviceId::new(
                crate::platform::HostId::CoreAudio,
                uid_string,
            ))
        } else {
            Err(ErrorKind::DeviceNotAvailable.into())
        }
    }

    // Logic re-used between `supported_input_configs` and `supported_output_configs`.
    #[expect(clippy::cast_ptr_alignment)]
    fn supported_configs(
        &self,
        scope: AudioObjectPropertyScope,
    ) -> Result<SupportedOutputConfigs, Error> {
        let mut property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
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

            // Get available sample rate ranges.
            property_address.mSelector = kAudioDevicePropertyAvailableNominalSampleRates;
            // SAFETY: kAudioDevicePropertyAvailableNominalSampleRates is documented to return an
            // array of AudioValueRange.
            let ranges: Vec<AudioValueRange> =
                get_property_array(self.audio_device_id, property_address)?;

            #[allow(non_upper_case_globals)]
            match scope {
                kAudioObjectPropertyScopeInput | kAudioObjectPropertyScopeOutput => {}
                _ => {
                    return Err(Error::with_message(
                        ErrorKind::UnsupportedOperation,
                        "Unexpected audio property scope",
                    ));
                }
            }
            let buffer_size = get_io_buffer_frame_size_range(self.audio_device_id)?;

            // AUHAL always converts to and from F32 regardless of the physical format, so
            // advertise it at every nominal rate (most hardware reports discrete rates, i.e.
            // mMinimum == mMaximum; some aggregate or virtual devices report continuous ranges).
            let f32_fmts = ranges.iter().map(|range| SupportedStreamConfigRange {
                channels: n_channels as ChannelCount,
                min_sample_rate: range.mMinimum as u32,
                max_sample_rate: range.mMaximum as u32,
                buffer_size,
                sample_format: SampleFormat::F32,
            });

            // The hardware's own physical formats, so integer-only devices advertise their
            // bit-perfect paths instead of only the AUHAL-converted F32 one.
            let physical_fmts = get_supported_physical_stream_formats(self.audio_device_id)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|fmt| {
                    let Some(coreaudio::audio_unit::AudioFormat::LinearPCM(flags)) =
                        coreaudio::audio_unit::AudioFormat::from_format_and_flag(
                            fmt.mFormat.mFormatID,
                            Some(fmt.mFormat.mFormatFlags),
                        )
                    else {
                        return None;
                    };
                    let sample_format = match CoreAudioSampleFormat::from_flags_and_bits_per_sample(
                        flags,
                        fmt.mFormat.mBitsPerChannel,
                    )? {
                        CoreAudioSampleFormat::I8 => SampleFormat::I8,
                        CoreAudioSampleFormat::I16 => SampleFormat::I16,
                        CoreAudioSampleFormat::I24 => SampleFormat::I24,
                        CoreAudioSampleFormat::I32 => SampleFormat::I32,
                        // Already covered by f32_fmts at every rate, not just this row's range.
                        CoreAudioSampleFormat::F32 => return None,
                    };
                    Some(SupportedStreamConfigRange {
                        channels: fmt.mFormat.mChannelsPerFrame as ChannelCount,
                        min_sample_rate: fmt.mSampleRateRange.mMinimum as u32,
                        max_sample_rate: fmt.mSampleRateRange.mMaximum as u32,
                        buffer_size,
                        sample_format,
                    })
                });

            Ok(f32_fmts
                .chain(physical_fmts)
                .collect::<Vec<_>>()
                .into_iter())
        }
    }

    fn supported_input_configs(&self) -> Result<SupportedOutputConfigs, Error> {
        self.supported_configs(kAudioObjectPropertyScopeInput)
    }

    fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, Error> {
        self.supported_configs(kAudioObjectPropertyScopeOutput)
    }

    fn default_config(
        &self,
        scope: AudioObjectPropertyScope,
    ) -> Result<SupportedStreamConfig, Error> {
        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamFormat,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        };

        unsafe {
            // SAFETY: kAudioDevicePropertyStreamFormat is documented to return an
            // AudioStreamBasicDescription.
            let asbd: AudioStreamBasicDescription =
                get_property(self.audio_device_id, property_address)?;

            let sample_format = {
                let audio_format = coreaudio::audio_unit::AudioFormat::from_format_and_flag(
                    asbd.mFormatID,
                    Some(asbd.mFormatFlags),
                );
                let flags = match audio_format {
                    Some(coreaudio::audio_unit::AudioFormat::LinearPCM(flags)) => flags,
                    _ => {
                        return Err(Error::with_message(
                            ErrorKind::UnsupportedConfig,
                            "Audio format is not linear PCM",
                        ));
                    }
                };
                let maybe_sample_format =
                    coreaudio::audio_unit::SampleFormat::from_flags_and_bits_per_sample(
                        flags,
                        asbd.mBitsPerChannel,
                    );
                match maybe_sample_format {
                    Some(coreaudio::audio_unit::SampleFormat::F32) => SampleFormat::F32,
                    Some(coreaudio::audio_unit::SampleFormat::I16) => SampleFormat::I16,
                    _ => {
                        return Err(Error::with_message(
                            ErrorKind::UnsupportedConfig,
                            "Sample format is not supported; supported formats are F32 and I16",
                        ));
                    }
                }
            };

            #[allow(non_upper_case_globals)]
            match scope {
                kAudioObjectPropertyScopeInput | kAudioObjectPropertyScopeOutput => {}
                _ => {
                    return Err(Error::with_message(
                        ErrorKind::UnsupportedOperation,
                        "Unexpected audio property scope",
                    ));
                }
            }
            let buffer_size = get_io_buffer_frame_size_range(self.audio_device_id)?;

            let config = SupportedStreamConfig {
                sample_rate: asbd.mSampleRate as _,
                channels: asbd.mChannelsPerFrame as _,
                buffer_size,
                sample_format,
            };
            Ok(config)
        }
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config(kAudioObjectPropertyScopeInput)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
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

impl Device {
    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        crate::validate_stream_config(&config)?;

        // One budget for every wait below, not one per step.
        let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));

        // Input is not automatically rerouted, so its buffer depth is constant and its timestamp monotonic.

        // Set the physical stream format (bit depth + sample rate) on the hardware device.
        // This avoids unnecessary format conversions, which is especially important on aggregate
        // devices. Falls back to sample-rate-only if no matching physical format is available, or
        // if the closest match found doesn't actually run at the requested rate.
        if !set_physical_format(
            self.audio_device_id,
            kAudioObjectPropertyScopeInput,
            config.sample_rate,
            config.channels,
            sample_format,
            deadline,
        )
        .is_ok_and(|asbd| (asbd.mSampleRate - config.sample_rate as f64).abs() < 1.0)
        {
            set_sample_rate(self.audio_device_id, config.sample_rate, deadline)?;
        }

        let mut loopback_aggregate: Option<LoopbackDevice> = None;
        let mut audio_unit = if self.supports_input() {
            audio_unit_from_device(self, AudioUnitMode::Input)?
        } else {
            loopback_aggregate.replace(LoopbackDevice::from_device(self)?);
            audio_unit_from_device(
                &loopback_aggregate.as_ref().unwrap().aggregate_device,
                AudioUnitMode::Input,
            )?
        };

        // The scope and element for working with a device's input stream.
        let scope = Scope::Output;
        let element = Element::Input;

        // Configure stream format and buffer size for predictable callback behavior.
        let effective_device_id = loopback_aggregate
            .as_ref()
            .map(|l| l.aggregate_device.audio_device_id)
            .unwrap_or(self.audio_device_id);
        configure_stream_format_and_buffer(
            &mut audio_unit,
            config,
            sample_format,
            scope,
            element,
            effective_device_id,
        )?;

        let error_callback: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));
        let error_callback_disconnect = error_callback.clone();
        let pending_xrun = Arc::new(AtomicBool::new(false));
        let pending_xrun_overload = pending_xrun.clone();

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let (bytes_per_channel, sample_rate, device_buffer_frames, extra_latency_frames) =
            setup_callback_vars(&audio_unit, config, sample_format, Scope::Input);

        // A resized device buffer changes this depth, and is then refreshed by DisconnectManager.
        let latency_frames = Arc::new(AtomicUsize::new(
            device_buffer_frames.map_or(0, |frames| frames + extra_latency_frames),
        ));
        let callback_latency_frames = latency_frames.clone();
        let draining = Arc::new(AtomicBool::new(false));
        let draining_input = draining.clone();

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

            if !draining_input.load(Ordering::Relaxed) {
                let callback = match host_time_to_stream_instant(args.time_stamp.mHostTime) {
                    Err(err) => {
                        let _ = try_emit_error(&error_callback, err);
                        return Err(());
                    }
                    Ok(cb) => cb,
                };
                let latency_frames = resolve_latency_frames(
                    &callback_latency_frames,
                    len,
                    channels as usize,
                    extra_latency_frames,
                );
                let delay = frames_to_duration(latency_frames as FrameCount, sample_rate);
                let capture = callback.checked_sub(delay).unwrap_or(StreamInstant::ZERO);
                let timestamp = StreamTimestamp {
                    callback,
                    device: capture,
                };
                let xrun = pending_xrun.swap(false, Ordering::Relaxed);
                data_callback(&data, &CallbackInfo { timestamp, xrun });
            }
            Ok(())
        })?;

        // All properties and callbacks are now configured on the uninitialized unit.
        // Initialize here so CoreAudio allocates its internal buffers for the actual format.
        audio_unit.initialize()?;

        let inner_arc = Arc::new(Mutex::new(StreamInner {
            playing: false,
            audio_unit,
            _device_id: self.audio_device_id,
            _loopback_device: loopback_aggregate,
        }));
        let weak_inner = Arc::downgrade(&inner_arc);
        let monitor: Box<dyn Monitor> = Box::new(DisconnectManager::new(
            self.audio_device_id,
            weak_inner,
            error_callback_disconnect,
            (latency_frames, Scope::Input),
            pending_xrun_overload,
        )?);
        // Capture never drains, so there are no frames to wait out.
        let stream = Stream::new(
            inner_arc,
            monitor,
            draining,
            Arc::new(AtomicUsize::new(0)),
            sample_rate,
        );
        stream.signal_ready();
        Ok(stream)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        crate::validate_stream_config(&config)?;

        // One budget for every wait below, not one per step.
        let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));

        // Keep `playback` monotonic: a default output device reroute can lower the device buffer depth,
        // pulling `playback` backward.
        let mut data_callback = crate::host::monotonic_output_callback(data_callback);

        // Best-effort: set the physical stream format (bit depth + sample rate) on the hardware.
        // This avoids unnecessary conversions, especially on aggregate devices. Not an error if
        // it fails: the AudioUnit will handle format conversion as before. Also falls back if the
        // closest match found doesn't actually run at the requested rate.
        if !set_physical_format(
            self.audio_device_id,
            kAudioObjectPropertyScopeOutput,
            config.sample_rate,
            config.channels,
            sample_format,
            deadline,
        )
        .is_ok_and(|asbd| (asbd.mSampleRate - config.sample_rate as f64).abs() < 1.0)
        {
            set_sample_rate(self.audio_device_id, config.sample_rate, deadline)?;
        }

        let mode = if self.is_default_output {
            AudioUnitMode::DefaultOutput
        } else {
            AudioUnitMode::Output
        };
        let mut audio_unit = audio_unit_from_device(self, mode)?;

        // The scope and element for working with a device's output stream.
        let scope = Scope::Input;
        let element = Element::Output;

        // Configure device buffer (see comprehensive documentation in input stream above)
        configure_stream_format_and_buffer(
            &mut audio_unit,
            config,
            sample_format,
            scope,
            element,
            self.audio_device_id,
        )?;

        let error_callback: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));
        let error_callback_for_render = error_callback.clone();
        let pending_xrun = Arc::new(AtomicBool::new(false));
        let pending_xrun_overload = pending_xrun.clone();

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let (bytes_per_channel, sample_rate, device_buffer_frames, extra_latency_frames) =
            setup_callback_vars(&audio_unit, config, sample_format, Scope::Output);

        // A DefaultOutput unit auto-reroutes to a new device without rebuilding the stream,
        // which changes this depth, and is then refreshed by DefaultOutputMonitor.
        let latency_frames = Arc::new(AtomicUsize::new(
            device_buffer_frames.map_or(0, |frames| frames + extra_latency_frames),
        ));
        let callback_latency_frames = latency_frames.clone();
        let draining = Arc::new(AtomicBool::new(false));
        let draining_render = draining.clone();
        let drain_frames = latency_frames.clone();

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

            let bytes = std::slice::from_raw_parts_mut(data as *mut u8, data_byte_size as usize);
            fill_equilibrium(bytes, sample_format);

            if draining_render.load(Ordering::Relaxed) {
                return Ok(());
            }

            let mut data = Data::from_parts(data, len, sample_format);

            let callback = match host_time_to_stream_instant(args.time_stamp.mHostTime) {
                Err(err) => {
                    let _ = try_emit_error(&error_callback_for_render, err);
                    return Err(());
                }
                Ok(cb) => cb,
            };
            let latency_frames = resolve_latency_frames(
                &callback_latency_frames,
                len,
                channels as usize,
                extra_latency_frames,
            );
            let delay = frames_to_duration(latency_frames as FrameCount, sample_rate);
            let playback = callback + delay;
            let timestamp = StreamTimestamp {
                callback,
                device: playback,
            };
            let xrun = pending_xrun.swap(false, Ordering::Relaxed);

            let info = CallbackInfo { timestamp, xrun };
            data_callback(&mut data, &info);
            Ok(())
        })?;

        // All properties and callbacks are now configured on the uninitialized unit.
        // Initialize here so CoreAudio allocates its internal buffers for the actual format.
        audio_unit.initialize()?;

        let inner_arc = Arc::new(Mutex::new(StreamInner {
            playing: false,
            audio_unit,
            _device_id: self.audio_device_id,
            _loopback_device: None,
        }));
        let weak_inner = Arc::downgrade(&inner_arc);
        let monitor: Box<dyn Monitor> = if matches!(mode, AudioUnitMode::DefaultOutput) {
            // Refresh the buffer depth whenever the default device reroutes automatically.
            Box::new(DefaultOutputMonitor::new(
                weak_inner,
                error_callback,
                (latency_frames.clone(), Scope::Output),
                pending_xrun_overload,
            )?)
        } else {
            // An explicit device never reroutes, so a resized buffer is the only depth change.
            Box::new(DisconnectManager::new(
                self.audio_device_id,
                weak_inner,
                error_callback,
                (latency_frames.clone(), Scope::Output),
                pending_xrun_overload,
            )?)
        };
        let stream = Stream::new(inner_arc, monitor, draining, drain_frames, sample_rate);
        stream.signal_ready();
        Ok(stream)
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.audio_device_id == other.audio_device_id
    }
}

impl Eq for Device {}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let desc = self.description().map_err(|_| fmt::Error)?;
        f.write_str(desc.name())
    }
}

impl std::hash::Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.audio_device_id.hash(state);
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("audio_device_id", &self.audio_device_id)
            .field("name", &self.description().map(|d| d.name().to_owned()))
            .finish()
    }
}

/// Configure stream format and buffer size for CoreAudio stream.
///
/// This handles the common setup tasks for both input and output streams:
/// - Sets the stream format (ASBD)
/// - Configures buffer size for Fixed buffer size requests
fn configure_stream_format_and_buffer(
    audio_unit: &mut AudioUnit,
    config: StreamConfig,
    sample_format: SampleFormat,
    scope: Scope,
    element: Element,
    device_id: AudioDeviceID,
) -> Result<(), Error> {
    // Set the stream format using stream-specific scope/element
    // - Input streams: scope=Output, element=Input (configuring output format of input element)
    // - Output streams: scope=Input, element=Output (configuring input format of output element)
    let asbd = asbd_from_config(config, sample_format);
    audio_unit.set_property(kAudioUnitProperty_StreamFormat, scope, element, Some(&asbd))?;

    // Configure device buffer size if requested
    if let BufferSize::Fixed(buffer_size) = config.buffer_size {
        // Pre-validate against the hardware range so callers get a human-readable error.
        if let Ok(SupportedBufferSize::Range { min, max }) =
            get_io_buffer_frame_size_range(device_id)
        {
            if !(min..=max).contains(&buffer_size) {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!(
                        "Buffer size {buffer_size} is not in the supported range {min}..={max}"
                    ),
                ));
            }
        }
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

/// Returns the sum of the device latency and safety offset in frames.
pub(crate) fn get_device_extra_latency_frames(audio_unit: &AudioUnit, scope: Scope) -> usize {
    let device_latency: u32 = audio_unit
        .get_property(kAudioDevicePropertyLatency, scope, Element::Output)
        .unwrap_or(0);
    let safety_offset: u32 = audio_unit
        .get_property(kAudioDevicePropertySafetyOffset, scope, Element::Output)
        .unwrap_or(0);
    (device_latency + safety_offset) as usize
}

/// Total buffer depth in frames: the IO buffer plus the device's own latency, or 0 if the
/// device buffer size cannot be queried.
pub(crate) fn device_latency_frames(audio_unit: &AudioUnit, scope: Scope) -> usize {
    get_device_buffer_frame_size(audio_unit)
        .ok()
        .map_or(0, |buffer| {
            buffer + get_device_extra_latency_frames(audio_unit, scope)
        })
}

/// Buffer depth for one callback.
///
/// Refreshed by the stream's monitor when the device buffer is resized; falls back to estimating
/// the device buffer from this callback, plus the fixed device latency and safety offset, when
/// the depth is unknown (zero).
#[inline]
fn resolve_latency_frames(
    cached: &AtomicUsize,
    len: usize,
    channels: usize,
    extra_latency_frames: usize,
) -> usize {
    match cached.load(Ordering::Relaxed) {
        0 => len.checked_div(channels).unwrap_or(0) + extra_latency_frames,
        n => n,
    }
}

/// Setup common callback variables, querying both the I/O buffer size and extra hardware latency.
///
/// Returns `(bytes_per_channel, sample_rate, device_buffer_frames, extra_latency_frames)`
fn setup_callback_vars(
    audio_unit: &AudioUnit,
    config: StreamConfig,
    sample_format: SampleFormat,
    scope: Scope,
) -> (usize, SampleRate, Option<usize>, usize) {
    let bytes_per_channel = sample_format.sample_size();
    let sample_rate = config.sample_rate;

    let device_buffer_frames = get_device_buffer_frame_size(audio_unit).ok();
    let extra_latency_frames = get_device_extra_latency_frames(audio_unit, scope);

    (
        bytes_per_channel,
        sample_rate,
        device_buffer_frames,
        extra_latency_frames,
    )
}

/// Query the current device buffer frame size from CoreAudio.
///
/// Buffer frame size is a device-level property that always uses Scope::Global + Element::Output,
/// regardless of whether the audio unit is configured for input or output streams.
pub(crate) fn get_device_buffer_frame_size(
    audio_unit: &AudioUnit,
) -> Result<usize, coreaudio::Error> {
    // Device-level property: always use Scope::Global + Element::Output
    // This is consistent with how we set the buffer size and query the buffer size range
    let frames: u32 = audio_unit.get_property(
        kAudioDevicePropertyBufferFrameSize,
        Scope::Global,
        Element::Output,
    )?;
    Ok(frames as usize)
}

//!
//! coreaudio on iOS looks a bit different from macOS. A lot of configuration needs to use
//! the AVAudioSession objc API which doesn't exist on macOS.
//!
//! TODO:
//! - Use AVAudioSession to enumerate buffer size / sample rate / number of channels and set
//!   buffer size.
//!

extern crate core_foundation_sys;
extern crate coreaudio;

use std::cell::RefCell;

use self::coreaudio::audio_unit::render_callback::data;
use self::coreaudio::audio_unit::{render_callback, AudioUnit, Element, Scope};
use self::coreaudio::sys::{
    kAudioOutputUnitProperty_EnableIO, kAudioUnitProperty_StreamFormat, AudioBuffer,
    AudioStreamBasicDescription,
};

use super::{asbd_from_config, frames_to_duration, host_time_to_stream_instant};
use crate::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};

use self::enumerate::{
    default_input_device, default_output_device, Devices, SupportedInputConfigs,
    SupportedOutputConfigs,
};
use std::slice;
use std::time::Duration;

pub mod enumerate;

// These days the default of iOS is now F32 and no longer I16
const SUPPORTED_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

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

impl Device {
    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("Default Device".to_owned())
    }

    #[inline]
    fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        // TODO: query AVAudioSession for parameters, some values like sample rate and buffer size
        // probably need to actually be set to see if it works, but channels can be enumerated.

        let asbd: AudioStreamBasicDescription = default_input_asbd()?;
        let stream_config = stream_config_from_asbd(asbd);
        Ok(vec![SupportedStreamConfigRange {
            channels: stream_config.channels,
            min_sample_rate: stream_config.sample_rate,
            max_sample_rate: stream_config.sample_rate,
            buffer_size: stream_config.buffer_size.clone(),
            sample_format: SUPPORTED_SAMPLE_FORMAT,
        }]
        .into_iter())
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        // TODO: query AVAudioSession for parameters, some values like sample rate and buffer size
        // probably need to actually be set to see if it works, but channels can be enumerated.

        let asbd: AudioStreamBasicDescription = default_output_asbd()?;
        let stream_config = stream_config_from_asbd(asbd);

        let configs: Vec<_> = (1..=asbd.mChannelsPerFrame as u16)
            .map(|channels| SupportedStreamConfigRange {
                channels,
                min_sample_rate: stream_config.sample_rate,
                max_sample_rate: stream_config.sample_rate,
                buffer_size: stream_config.buffer_size.clone(),
                sample_format: SUPPORTED_SAMPLE_FORMAT,
            })
            .collect();
        Ok(configs.into_iter())
    }

    #[inline]
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let asbd: AudioStreamBasicDescription = default_input_asbd()?;
        let stream_config = stream_config_from_asbd(asbd);
        Ok(stream_config)
    }

    #[inline]
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let asbd: AudioStreamBasicDescription = default_output_asbd()?;
        let stream_config = stream_config_from_asbd(asbd);
        Ok(stream_config)
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    #[inline]
    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Device::supported_input_configs(self)
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Device::supported_output_configs(self)
    }

    #[inline]
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_input_config(self)
    }

    #[inline]
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        mut error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // The scope and element for working with a device's input stream.
        let scope = Scope::Output;
        let element = Element::Input;

        let mut audio_unit = create_audio_unit()?;
        audio_unit.uninitialize()?;
        configure_for_recording(&mut audio_unit)?;
        audio_unit.initialize()?;

        // Set the stream in interleaved mode.
        let asbd = asbd_from_config(config, sample_format);
        audio_unit.set_property(kAudioUnitProperty_StreamFormat, scope, element, Some(&asbd))?;

        // Set the buffersize
        match config.buffer_size {
            BufferSize::Fixed(_) => {
                return Err(BuildStreamError::StreamConfigNotSupported);
            }
            BufferSize::Default => (),
        }

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let bytes_per_channel = sample_format.sample_size();
        let sample_rate = config.sample_rate;
        type Args = render_callback::Args<data::Raw>;
        audio_unit.set_input_callback(move |args: Args| unsafe {
            let ptr = (*args.data.data).mBuffers.as_ptr() as *const AudioBuffer;
            let len = (*args.data.data).mNumberBuffers as usize;
            let buffers: &[AudioBuffer] = slice::from_raw_parts(ptr, len);

            // There is only 1 buffer when using interleaved channels
            let AudioBuffer {
                mNumberChannels: channels,
                mDataByteSize: data_byte_size,
                mData: data,
            } = buffers[0];

            let data = data as *mut ();
            let len = (data_byte_size as usize / bytes_per_channel) as usize;
            let data = Data::from_parts(data, len, sample_format);

            // TODO: Need a better way to get delay, for now we assume a double-buffer offset.
            let callback = match host_time_to_stream_instant(args.time_stamp.mHostTime) {
                Err(err) => {
                    error_callback(err.into());
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

        audio_unit.start()?;

        Ok(Stream::new(StreamInner {
            playing: true,
            audio_unit,
        }))
    }

    /// Create an output stream.
    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        mut error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        match config.buffer_size {
            BufferSize::Fixed(_) => {
                return Err(BuildStreamError::StreamConfigNotSupported);
            }
            BufferSize::Default => (),
        };

        let mut audio_unit = create_audio_unit()?;

        // The scope and element for working with a device's output stream.
        let scope = Scope::Input;
        let element = Element::Output;

        // Set the stream in interleaved mode.
        let asbd = asbd_from_config(config, sample_format);
        audio_unit.set_property(kAudioUnitProperty_StreamFormat, scope, element, Some(&asbd))?;

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
            let len = (data_byte_size as usize / bytes_per_channel) as usize;
            let mut data = Data::from_parts(data, len, sample_format);

            let callback = match host_time_to_stream_instant(args.time_stamp.mHostTime) {
                Err(err) => {
                    error_callback(err.into());
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

        audio_unit.start()?;

        Ok(Stream::new(StreamInner {
            playing: true,
            audio_unit,
        }))
    }
}

pub struct Stream {
    inner: RefCell<StreamInner>,
}

impl Stream {
    fn new(inner: StreamInner) -> Self {
        Self {
            inner: RefCell::new(inner),
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let mut stream = self.inner.borrow_mut();

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
        let mut stream = self.inner.borrow_mut();

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

struct StreamInner {
    playing: bool,
    audio_unit: AudioUnit,
}

fn create_audio_unit() -> Result<AudioUnit, coreaudio::Error> {
    AudioUnit::new(coreaudio::audio_unit::IOType::RemoteIO)
}

fn configure_for_recording(audio_unit: &mut AudioUnit) -> Result<(), coreaudio::Error> {
    // Enable mic recording
    let enable_input = 1u32;
    audio_unit.set_property(
        kAudioOutputUnitProperty_EnableIO,
        Scope::Input,
        Element::Input,
        Some(&enable_input),
    )?;

    // Disable output
    let disable_output = 0u32;
    audio_unit.set_property(
        kAudioOutputUnitProperty_EnableIO,
        Scope::Output,
        Element::Output,
        Some(&disable_output),
    )?;

    Ok(())
}

fn default_output_asbd() -> Result<AudioStreamBasicDescription, coreaudio::Error> {
    let audio_unit = create_audio_unit()?;
    let id = kAudioUnitProperty_StreamFormat;
    let asbd: AudioStreamBasicDescription =
        audio_unit.get_property(id, Scope::Output, Element::Output)?;
    Ok(asbd)
}

fn default_input_asbd() -> Result<AudioStreamBasicDescription, coreaudio::Error> {
    let mut audio_unit = create_audio_unit()?;
    audio_unit.uninitialize()?;
    configure_for_recording(&mut audio_unit)?;
    audio_unit.initialize()?;

    let id = kAudioUnitProperty_StreamFormat;
    let asbd: AudioStreamBasicDescription =
        audio_unit.get_property(id, Scope::Input, Element::Input)?;
    Ok(asbd)
}

fn stream_config_from_asbd(asbd: AudioStreamBasicDescription) -> SupportedStreamConfig {
    let buffer_size = SupportedBufferSize::Range { min: 0, max: 0 };
    SupportedStreamConfig {
        channels: asbd.mChannelsPerFrame as u16,
        sample_rate: SampleRate(asbd.mSampleRate as u32),
        buffer_size: buffer_size.clone(),
        sample_format: SUPPORTED_SAMPLE_FORMAT,
    }
}

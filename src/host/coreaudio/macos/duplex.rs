use super::{
    asbd_from_config, frames_to_duration, host_time_to_stream_instant, invoke_error_callback,
    DuplexCallbackPtr, Stream, StreamInner,
};
use crate::duplex::DuplexCallbackInfo;
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, SampleFormat, StreamConfig,
    StreamError,
};
use coreaudio::audio_unit::{AudioUnit, Element, Scope};
use objc2_audio_toolbox::{
    kAudioOutputUnitProperty_CurrentDevice, kAudioOutputUnitProperty_EnableIO,
    kAudioUnitProperty_SetRenderCallback, kAudioUnitProperty_StreamFormat, AURenderCallbackStruct,
    AudioUnitRender, AudioUnitRenderActionFlags,
};
use objc2_core_audio::kAudioDevicePropertyBufferFrameSize;
use objc2_core_audio_types::{kAudio_ParamError, AudioBuffer, AudioBufferList, AudioTimeStamp};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use super::device::{
    estimate_capture_instant, estimate_playback_instant, get_device_buffer_frame_size,
    set_sample_rate, Device, AUDIO_UNIT_IO_ENABLED,
};
use crate::traits::DeviceTrait;

type DuplexProcFn = dyn FnMut(
    NonNull<AudioUnitRenderActionFlags>,
    NonNull<AudioTimeStamp>,
    u32, // bus_number
    u32, // num_frames
    *mut AudioBufferList,
) -> i32;

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

// `extern "C-unwind"` matches `AURenderCallbackStruct::inputProc`.
// `catch_unwind` prevents panics from unwinding through CoreAudio's C frames.
extern "C-unwind" fn duplex_input_proc(
    in_ref_con: NonNull<c_void>,
    io_action_flags: NonNull<AudioUnitRenderActionFlags>,
    in_time_stamp: NonNull<AudioTimeStamp>,
    in_bus_number: u32,
    in_number_frames: u32,
    io_data: *mut AudioBufferList,
) -> i32 {
    // SAFETY: `in_ref_con` originates from `Box::into_raw` in `build_duplex_stream_raw`.
    // `StreamInner::drop` stops the audio unit before reclaiming the pointer,
    // so it remains valid for the lifetime of the callback.
    // Called from a single render thread per audio unit, so `as_mut()` has exclusive access.
    let wrapper = unsafe { in_ref_con.cast::<DuplexProcWrapper>().as_mut() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (wrapper.callback)(
            io_action_flags,
            in_time_stamp,
            in_bus_number,
            in_number_frames,
            io_data,
        )
    })) {
        Ok(result) => result,
        Err(_) => kAudio_ParamError,
    }
}

impl Device {
    // See: https://developer.apple.com/library/archive/technotes/tn2091/_index.html
    pub(crate) fn build_duplex_stream_raw<D, E>(
        &self,
        config: &crate::duplex::DuplexStreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        if !self.supports_duplex() {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        set_sample_rate(self.audio_device_id, config.sample_rate)?;

        let mut audio_unit = AudioUnit::new(coreaudio::audio_unit::IOType::HalOutput)?;

        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Input,
            Element::Input,
            Some(&AUDIO_UNIT_IO_ENABLED),
        )?;

        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Output,
            Element::Output,
            Some(&AUDIO_UNIT_IO_ENABLED),
        )?;

        audio_unit.set_property(
            kAudioOutputUnitProperty_CurrentDevice,
            Scope::Global,
            Element::Output,
            Some(&self.audio_device_id),
        )?;

        let input_stream_config = StreamConfig {
            channels: config.input_channels,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };

        let output_stream_config = StreamConfig {
            channels: config.output_channels,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };

        // Client-side format: Scope::Output for input bus, Scope::Input for output bus.
        let input_asbd = asbd_from_config(input_stream_config, sample_format);
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Output,
            Element::Input,
            Some(&input_asbd),
        )?;

        let output_asbd = asbd_from_config(output_stream_config, sample_format);
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&output_asbd),
        )?;

        if let BufferSize::Fixed(buffer_size) = &config.buffer_size {
            audio_unit.set_property(
                kAudioDevicePropertyBufferFrameSize,
                Scope::Global,
                Element::Output,
                Some(buffer_size),
            )?;
        }

        let current_buffer_size = get_device_buffer_frame_size(&audio_unit).map_err(|e| {
            BuildStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: e.to_string(),
                },
            }
        })?;

        let sample_rate = config.sample_rate;
        let device_buffer_frames = current_buffer_size;
        let raw_audio_unit = *audio_unit.as_ref();
        let input_channels = config.input_channels as usize;
        let sample_bytes = sample_format.sample_size();

        let input_buffer_bytes = current_buffer_size * input_channels * sample_bytes;
        let mut input_buffer: Box<[u8]> = vec![0u8; input_buffer_bytes].into_boxed_slice();

        let error_callback = Arc::new(Mutex::new(error_callback));
        let error_callback_for_callback = error_callback.clone();

        let mut data_callback = data_callback;
        let buffer_size_changed = std::sync::atomic::AtomicBool::new(false);

        let duplex_proc: Box<DuplexProcFn> = Box::new(
            move |io_action_flags: NonNull<AudioUnitRenderActionFlags>,
                  in_time_stamp: NonNull<AudioTimeStamp>,
                  _in_bus_number: u32,
                  in_number_frames: u32,
                  io_data: *mut AudioBufferList|
                  -> i32 {
                if buffer_size_changed.load(std::sync::atomic::Ordering::Relaxed) {
                    return kAudio_ParamError;
                }

                if io_data.is_null() {
                    return kAudio_ParamError;
                }
                // SAFETY: io_data validated as non-null above.
                let buffer_list = unsafe { &mut *io_data };
                if buffer_list.mNumberBuffers == 0 {
                    return kAudio_ParamError;
                }

                let num_frames = in_number_frames as usize;
                let input_samples = num_frames * input_channels;
                let input_bytes = input_samples * sample_bytes;

                if input_bytes != input_buffer.len() {
                    buffer_size_changed.store(true, std::sync::atomic::Ordering::Relaxed);
                    return kAudio_ParamError;
                }

                // SAFETY: in_time_stamp is valid per CoreAudio callback contract.
                let timestamp: &AudioTimeStamp = unsafe { in_time_stamp.as_ref() };

                let callback_instant = match host_time_to_stream_instant(timestamp.mHostTime) {
                    Err(err) => {
                        invoke_error_callback(&error_callback_for_callback, err.into());
                        return 0;
                    }
                    Ok(cb) => cb,
                };

                let buffer = &mut buffer_list.mBuffers[0];
                if buffer.mData.is_null() {
                    return kAudio_ParamError;
                }
                let output_samples = buffer.mDataByteSize as usize / sample_bytes;

                // SAFETY: buffer.mData validated as non-null above.
                let mut output_data = unsafe {
                    Data::from_parts(buffer.mData as *mut (), output_samples, sample_format)
                };

                let delay = frames_to_duration(device_buffer_frames, sample_rate);

                let capture =
                    estimate_capture_instant(callback_instant, delay, &error_callback_for_callback);
                let playback = estimate_playback_instant(
                    callback_instant,
                    delay,
                    &error_callback_for_callback,
                );

                let input_timestamp = crate::InputStreamTimestamp {
                    callback: callback_instant,
                    capture,
                };
                let output_timestamp = crate::OutputStreamTimestamp {
                    callback: callback_instant,
                    playback,
                };

                let mut input_buffer_list = AudioBufferList {
                    mNumberBuffers: 1,
                    mBuffers: [AudioBuffer {
                        mNumberChannels: input_channels as u32,
                        mDataByteSize: input_bytes as u32,
                        mData: input_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                    }],
                };

                // SAFETY: raw_audio_unit is valid for the callback duration,
                // input_buffer_list points to bounds-checked input_buffer.
                let status = unsafe {
                    AudioUnitRender(
                        raw_audio_unit,
                        io_action_flags.as_ptr(),
                        in_time_stamp,
                        1, // Element 1 = input
                        in_number_frames,
                        NonNull::new_unchecked(&mut input_buffer_list),
                    )
                };

                if status != 0 {
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

                // SAFETY: input_buffer is bounds-checked, filled by AudioUnitRender
                // (or zeroed on error), and outlives this Data reference.
                let input_data = unsafe {
                    Data::from_parts(
                        input_buffer.as_mut_ptr() as *mut (),
                        input_samples,
                        sample_format,
                    )
                };

                let callback_info = DuplexCallbackInfo::new(input_timestamp, output_timestamp);
                data_callback(&input_data, &mut output_data, &callback_info);

                0
            },
        );

        let wrapper = Box::new(DuplexProcWrapper {
            callback: duplex_proc,
        });
        let wrapper_ptr = Box::into_raw(wrapper);

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

        let inner = StreamInner {
            playing: true,
            audio_unit: ManuallyDrop::new(audio_unit),
            device_id: self.audio_device_id,
            _loopback_device: None,
            duplex_callback_ptr: Some(DuplexCallbackPtr(wrapper_ptr)),
        };

        let error_callback_clone = error_callback.clone();
        let error_callback_for_stream: super::ErrorCallback = Box::new(move |err: StreamError| {
            invoke_error_callback(&error_callback_clone, err);
        });

        let stream = Stream::new(inner, error_callback_for_stream, true)?;

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
}

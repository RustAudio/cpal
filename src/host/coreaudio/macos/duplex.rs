use std::{
    ffi::c_void,
    mem::ManuallyDrop,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use coreaudio::audio_unit::{AudioUnit, Element, Scope};
use objc2_audio_toolbox::{
    AURenderCallbackStruct, AudioUnitRender, AudioUnitRenderActionFlags,
    kAudioOutputUnitProperty_CurrentDevice, kAudioOutputUnitProperty_EnableIO,
    kAudioUnitProperty_SetRenderCallback, kAudioUnitProperty_StreamFormat,
};
use objc2_core_audio::kAudioDevicePropertyBufferFrameSize;
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp, kAudio_ParamError};

use super::{
    DisconnectManager, DuplexCallbackPtr, Monitor, Stream, StreamInner, asbd_from_config,
    device::{
        Device, estimate_capture_instant, estimate_playback_instant, get_device_buffer_frame_size,
        set_sample_rate,
    },
    host_time_to_stream_instant,
};
use crate::{
    BufferSize, CallbackInfo, Data, DuplexCallbackInfo, DuplexStreamConfig, Error, ErrorKind,
    FrameCount, SampleFormat, StreamConfig, StreamTimestamp,
    host::{ErrorCallbackArc, equilibrium::fill_equilibrium, frames_to_duration, try_emit_error},
    traits::DeviceTrait,
};

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
    match catch_unwind(AssertUnwindSafe(|| {
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
        config: DuplexStreamConfig,
        input_sample_format: SampleFormat,
        output_sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        if !self.supports_duplex() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "device does not support both input and output",
            ));
        }

        set_sample_rate(self.audio_device_id, config.sample_rate, timeout)?;

        let mut audio_unit =
            AudioUnit::new_uninitialized(coreaudio::audio_unit::IOType::HalOutput)?;

        const ENABLED: u32 = 1;
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Input,
            Element::Input,
            Some(&ENABLED),
        )?;

        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Output,
            Element::Output,
            Some(&ENABLED),
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
        let input_asbd = asbd_from_config(input_stream_config, input_sample_format);
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Output,
            Element::Input,
            Some(&input_asbd),
        )?;

        let output_asbd = asbd_from_config(output_stream_config, output_sample_format);
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

        audio_unit.initialize()?;

        let current_buffer_size = get_device_buffer_frame_size(&audio_unit).map_err(|e| {
            Error::with_message(
                ErrorKind::BackendError,
                format!("failed to query device buffer size: {e}"),
            )
        })?;

        let sample_rate = config.sample_rate;
        let device_buffer_frames = current_buffer_size;
        let raw_audio_unit = *audio_unit.as_ref();
        let input_channels = config.input_channels as usize;
        let input_sample_bytes = input_sample_format.sample_size();
        let output_sample_bytes = output_sample_format.sample_size();

        let input_buffer_bytes = current_buffer_size * input_channels * input_sample_bytes;
        let mut input_buffer: Box<[u8]> = vec![0u8; input_buffer_bytes].into_boxed_slice();

        let error_callback: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));
        let error_callback_for_callback = error_callback.clone();

        let pending_xrun = Arc::new(AtomicBool::new(false));
        let pending_xrun_overload = pending_xrun.clone();
        let draining = Arc::new(AtomicBool::new(false));
        let draining_render = draining.clone();
        let drain_window = frames_to_duration(current_buffer_size as FrameCount, sample_rate);

        let mut data_callback = data_callback;
        let buffer_size_changed = AtomicBool::new(false);

        let duplex_proc: Box<DuplexProcFn> = Box::new(
            move |io_action_flags: NonNull<AudioUnitRenderActionFlags>,
                  in_time_stamp: NonNull<AudioTimeStamp>,
                  _in_bus_number: u32,
                  in_number_frames: u32,
                  io_data: *mut AudioBufferList|
                  -> i32 {
                if buffer_size_changed.load(Ordering::Relaxed) {
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
                let input_bytes = input_samples * input_sample_bytes;

                if input_bytes != input_buffer.len() {
                    buffer_size_changed.store(true, Ordering::Relaxed);
                    return kAudio_ParamError;
                }

                let buffer = &mut buffer_list.mBuffers[0];
                if buffer.mData.is_null() {
                    return kAudio_ParamError;
                }
                let output_byte_len = buffer.mDataByteSize as usize;
                let output_samples = output_byte_len / output_sample_bytes;

                // Pre-fill the output with silence so any early return below (draining,
                // timestamp failure) plays silence rather than stale buffer contents.
                // SAFETY: buffer.mData validated as non-null above; CoreAudio guarantees the
                // buffer holds mDataByteSize bytes.
                unsafe {
                    let bytes =
                        std::slice::from_raw_parts_mut(buffer.mData as *mut u8, output_byte_len);
                    fill_equilibrium(bytes, output_sample_format);
                }

                if draining_render.load(Ordering::Relaxed) {
                    return 0;
                }

                // SAFETY: in_time_stamp is valid per CoreAudio callback contract.
                let timestamp: &AudioTimeStamp = unsafe { in_time_stamp.as_ref() };

                let callback_instant = match host_time_to_stream_instant(timestamp.mHostTime) {
                    Err(err) => {
                        let _ = try_emit_error(&error_callback_for_callback, err);
                        return 0;
                    }
                    Ok(cb) => cb,
                };

                // SAFETY: buffer.mData validated as non-null above.
                let mut output_data = unsafe {
                    Data::from_parts(
                        buffer.mData as *mut (),
                        output_samples,
                        output_sample_format,
                    )
                };

                let delay = frames_to_duration(device_buffer_frames as FrameCount, sample_rate);

                let capture =
                    estimate_capture_instant(callback_instant, delay, &error_callback_for_callback);
                let playback = estimate_playback_instant(
                    callback_instant,
                    delay,
                    &error_callback_for_callback,
                );

                // A processor-overload notification does not say which direction glitched, so
                // report it on both.
                let xrun = pending_xrun.swap(false, Ordering::Relaxed);
                let input_info = CallbackInfo {
                    timestamp: StreamTimestamp {
                        callback: callback_instant,
                        device: capture,
                    },
                    xrun,
                };
                let output_info = CallbackInfo {
                    timestamp: StreamTimestamp {
                        callback: callback_instant,
                        device: playback,
                    },
                    xrun,
                };

                let mut input_buffer_list = AudioBufferList {
                    mNumberBuffers: 1,
                    mBuffers: [AudioBuffer {
                        mNumberChannels: input_channels as u32,
                        mDataByteSize: input_bytes as u32,
                        mData: input_buffer.as_mut_ptr() as *mut c_void,
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
                    let _ = try_emit_error(
                        &error_callback_for_callback,
                        Error::with_message(
                            ErrorKind::BackendError,
                            format!("AudioUnitRender failed for input: OSStatus {status}"),
                        ),
                    );
                    input_buffer[..input_bytes].fill(0);
                }

                // SAFETY: input_buffer is bounds-checked, filled by AudioUnitRender
                // (or zeroed on error), and outlives this Data reference.
                let input_data = unsafe {
                    Data::from_parts(
                        input_buffer.as_mut_ptr() as *mut (),
                        input_samples,
                        input_sample_format,
                    )
                };

                let callback_info = DuplexCallbackInfo::new(input_info, output_info);
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
            inputProcRefCon: wrapper_ptr as *mut c_void,
        };

        audio_unit.set_property(
            kAudioUnitProperty_SetRenderCallback,
            Scope::Global,
            Element::Output,
            Some(&render_callback),
        )?;

        let inner_arc = Arc::new(Mutex::new(StreamInner {
            playing: false,
            audio_unit: ManuallyDrop::new(audio_unit),
            _device_id: self.audio_device_id,
            _loopback_device: None,
            duplex_callback_ptr: Some(DuplexCallbackPtr(wrapper_ptr)),
        }));
        let weak_inner = Arc::downgrade(&inner_arc);
        let monitor: Box<dyn Monitor> = Box::new(DisconnectManager::new(
            self.audio_device_id,
            weak_inner,
            error_callback,
            pending_xrun_overload,
            true,
        )?);

        let stream = Stream::new(inner_arc, monitor, draining, drain_window);
        stream.signal_ready();
        Ok(stream)
    }
}

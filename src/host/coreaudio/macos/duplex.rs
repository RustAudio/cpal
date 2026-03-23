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
/// We use `catch_unwind` to prevent panics from unwinding through CoreAudio's
/// C frames, which would be undefined behavior.
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
    /// Build a duplex stream with synchronized input and output.
    ///
    /// This creates a single HAL AudioUnit with both input and output enabled,
    /// ensuring they share the same hardware clock.
    /// For details, see: https://developer.apple.com/library/archive/technotes/tn2091/_index.html
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
        // Validate that device supports duplex
        if !self.supports_duplex() {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        // Potentially change the device sample rate to match the config.
        set_sample_rate(self.audio_device_id, config.sample_rate)?;

        // Create HAL AudioUnit - always use HalOutput for duplex
        let mut audio_unit = AudioUnit::new(coreaudio::audio_unit::IOType::HalOutput)?;

        // Enable BOTH input and output on the AudioUnit
        // Enable input on Element 1
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Input,
            Element::Input,
            Some(&AUDIO_UNIT_IO_ENABLED),
        )?;

        // Enable output on Element 0 (usually enabled by default, but be explicit)
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Output,
            Element::Output,
            Some(&AUDIO_UNIT_IO_ENABLED),
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
            channels: config.input_channels,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };

        // Create StreamConfig for output side
        let output_stream_config = StreamConfig {
            channels: config.output_channels,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };

        // Core Audio's HAL AU has two buses, each with a hardware side and a
        // client (app) side. We set the stream format on the client-facing side;
        // the AU's built-in converter handles translation to/from the hardware format.
        //
        //   Mic ─[Scope::Input]──▶ Input Bus ──[Scope::Output]─▶ App
        //        (hardware side)                (client side)
        //
        //   App ─[Scope::Input]──▶ Output Bus ─[Scope::Output]─▶ Speaker
        //        (client side)                   (hardware side)
        //
        // So the client side is Scope::Output for the input bus (where we read
        // captured samples) and Scope::Input for the output bus (where we write
        // playback samples). See Apple TN2091 for details.
        let input_asbd = asbd_from_config(&input_stream_config, sample_format);
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Output,
            Element::Input,
            Some(&input_asbd),
        )?;

        let output_asbd = asbd_from_config(&output_stream_config, sample_format);
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&output_asbd),
        )?;

        // Buffer frame size is a device-level property. Element::Output (0) is
        // the standard convention for Scope::Global properties.
        if let BufferSize::Fixed(buffer_size) = &config.buffer_size {
            audio_unit.set_property(
                kAudioDevicePropertyBufferFrameSize,
                Scope::Global,
                Element::Output,
                Some(buffer_size),
            )?;
        }

        // Allocate input buffer for the current device buffer size.
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

        // Wrap error callback in Arc<Mutex> for sharing between callback and disconnect handler
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
                // SAFETY: io_data validated as non-null above
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

                // SAFETY: in_time_stamp is valid per CoreAudio contract
                let timestamp: &AudioTimeStamp = unsafe { in_time_stamp.as_ref() };

                // Create StreamInstant for callback_instant
                let callback_instant = match host_time_to_stream_instant(timestamp.mHostTime) {
                    Err(err) => {
                        invoke_error_callback(&error_callback_for_callback, err.into());
                        // Return 0 (noErr) to keep the stream alive while notifying the error
                        // callback. This matches input/output stream behavior and allows graceful
                        // degradation rather than stopping the stream on transient errors.
                        return 0;
                    }
                    Ok(cb) => cb,
                };

                let buffer = &mut buffer_list.mBuffers[0];
                if buffer.mData.is_null() {
                    return kAudio_ParamError;
                }
                let output_samples = buffer.mDataByteSize as usize / sample_bytes;

                // SAFETY: buffer.mData validated as non-null above
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

                // Create callback info with latency-adjusted times
                let input_timestamp = crate::InputStreamTimestamp {
                    callback: callback_instant,
                    capture,
                };
                let output_timestamp = crate::OutputStreamTimestamp {
                    callback: callback_instant,
                    playback,
                };

                // Pull input from Element 1 using AudioUnitRender
                // use the pre-allocated input_buffer
                // Set up AudioBufferList pointing to our input buffer
                let mut input_buffer_list = AudioBufferList {
                    mNumberBuffers: 1,
                    mBuffers: [AudioBuffer {
                        mNumberChannels: input_channels as u32,
                        mDataByteSize: input_bytes as u32,
                        mData: input_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                    }],
                };

                // SAFETY: AudioUnitRender is called with valid parameters:
                // - raw_audio_unit is valid for the callback duration
                // - input_buffer_list is created just above on the stack with a pointer to
                //   input_buffer, which is properly aligned (Rust's standard allocators return
                //   pointers aligned to at least 8 bytes, exceeding f32/i16 requirements)
                // - input_buffer has been bounds-checked above to ensure sufficient capacity
                // - All other parameters (timestamps, flags, etc.) come from CoreAudio itself
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
                    // Report error but continue with silence for graceful degradation
                    // The application should decide what to do.
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

                // SAFETY: Creating Data from input_buffer is safe because:
                // - input_buffer is a valid Vec<u8> owned by this closure
                // - input_samples (num_frames * input_channels) was bounds-checked above to ensure
                //   input_samples * sample_bytes <= input_buffer.len()
                // - AudioUnitRender just filled the buffer with valid audio data (or we filled
                //   it with silence on error)
                // - The Data lifetime is scoped to this callback and doesn't outlive input_buffer
                // - The pointer is suitably aligned: We successfully passed this buffer to
                //   AudioUnitRender (line 1314), which requires properly aligned buffers and would
                //   have failed if alignment were incorrect. Additionally, in practice, Rust's
                //   standard allocators (System, jemalloc, etc.) return pointers aligned to at
                //   least 8 bytes, which exceeds the requirements for f32 (4 bytes) and i16 (2 bytes).
                let input_data = unsafe {
                    Data::from_parts(
                        input_buffer.as_mut_ptr() as *mut (),
                        input_samples,
                        sample_format,
                    )
                };

                let callback_info = DuplexCallbackInfo::new(input_timestamp, output_timestamp);
                data_callback(&input_data, &mut output_data, &callback_info);

                // Return 0 (noErr) to indicate successful render
                0
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
            audio_unit: ManuallyDrop::new(audio_unit),
            device_id: self.audio_device_id,
            _loopback_device: None,
            duplex_callback_ptr: Some(DuplexCallbackPtr(wrapper_ptr)),
        };

        // Always propagate disconnect errors for duplex streams. A duplex stream
        // is broken when either direction changes device.
        let error_callback_clone = error_callback.clone();
        let error_callback_for_stream: super::ErrorCallback = Box::new(move |err: StreamError| {
            invoke_error_callback(&error_callback_clone, err);
        });

        // Create the duplex stream
        let stream = Stream::new(inner, error_callback_for_stream, true)?;

        // Start the audio unit
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

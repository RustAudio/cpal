use super::Endpoint;
use super::check_result;
use super::com;
use super::kernel32;
use super::ole32;
use super::winapi;

use std::marker::PhantomData;
use std::mem;
use std::ptr;
use std::slice;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use CreationError;
use Format;
use SampleFormat;
use UnknownTypeBuffer;

pub struct EventLoop {
    // Data used by the `run()` function implementation. The mutex is kept lock permanently by
    // `run()`. This ensures that two `run()` invocations can't run at the same time, and also
    // means that we shouldn't try to lock this field from anywhere else but `run()`.
    run_context: Mutex<RunContext>,

    // Identifier of the next voice to create. Each new voice increases this counter. If the
    // counter overflows, there's a panic.
    // TODO: use AtomicU64 instead
    next_voice_id: AtomicUsize,

    // Commands processed by the `run()` method that is currently running.
    // `pending_scheduled_event` must be signalled whenever a command is added here, so that it
    // will get picked up.
    // TODO: use a lock-free container
    commands: Mutex<Vec<Command>>,

    // This event is signalled after a new entry is added to `commands`, so that the `run()`
    // method can be notified.
    pending_scheduled_event: winapi::HANDLE,
}

struct RunContext {
    // Voices that have been created in this event loop.
    voices: Vec<VoiceInner>,

    // Handles corresponding to the `event` field of each element of `voices`. Must always be in
    // sync with `voices`, except that the first element is always `pending_scheduled_event`.
    handles: Vec<winapi::HANDLE>,
}

enum Command {
    NewVoice(VoiceInner),
    DestroyVoice(VoiceId),
    Play(VoiceId),
    Pause(VoiceId),
}

struct VoiceInner {
    id: VoiceId,
    audio_client: *mut winapi::IAudioClient,
    render_client: *mut winapi::IAudioRenderClient,
    // Event that is signalled by WASAPI whenever audio data must be written.
    event: winapi::HANDLE,
    // True if the voice is currently playing. False if paused.
    playing: bool,

    // Number of frames of audio data in the underlying buffer allocated by WASAPI.
    max_frames_in_buffer: winapi::UINT32,
    // Number of bytes that each frame occupies.
    bytes_per_frame: winapi::WORD,
}

impl EventLoop {
    pub fn new() -> EventLoop {
        let pending_scheduled_event =
            unsafe { kernel32::CreateEventA(ptr::null_mut(), 0, 0, ptr::null()) };

        EventLoop {
            pending_scheduled_event: pending_scheduled_event,
            run_context: Mutex::new(RunContext {
                                        voices: Vec::new(),
                                        handles: vec![pending_scheduled_event],
                                    }),
            next_voice_id: AtomicUsize::new(0),
            commands: Mutex::new(Vec::new()),
        }
    }

    pub fn build_voice(&self, end_point: &Endpoint, format: &Format)
                       -> Result<VoiceId, CreationError> {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match end_point.build_audioclient() {
                Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    return Err(CreationError::DeviceNotAvailable),
                e => e.unwrap(),
            };

            // Computing the format and initializing the device.
            let format = {
                let format_attempt = format_to_waveformatextensible(format)?;
                let share_mode = winapi::AUDCLNT_SHAREMODE_SHARED;

                // `IsFormatSupported` checks whether the format is supported and fills
                // a `WAVEFORMATEX`
                let mut dummy_fmt_ptr: *mut winapi::WAVEFORMATEX = mem::uninitialized();
                let hresult =
                    (*audio_client)
                        .IsFormatSupported(share_mode, &format_attempt.Format, &mut dummy_fmt_ptr);
                // we free that `WAVEFORMATEX` immediately after because we don't need it
                if !dummy_fmt_ptr.is_null() {
                    ole32::CoTaskMemFree(dummy_fmt_ptr as *mut _);
                }

                // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
                // has been found) but we also treat this as an error
                match (hresult, check_result(hresult)) {
                    (_, Err(ref e))
                        if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    (_, Err(e)) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    (winapi::S_FALSE, _) => {
                        (*audio_client).Release();
                        return Err(CreationError::FormatNotSupported);
                    },
                    (_, Ok(())) => (),
                };

                // finally initializing the audio client
                let hresult = (*audio_client).Initialize(share_mode,
                                                         winapi::AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                                         0,
                                                         0,
                                                         &format_attempt.Format,
                                                         ptr::null());
                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    Ok(()) => (),
                };

                format_attempt.Format
            };

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event = {
                let event = kernel32::CreateEventA(ptr::null_mut(), 0, 0, ptr::null());
                if event == ptr::null_mut() {
                    (*audio_client).Release();
                    panic!("Failed to create event");
                }

                match check_result((*audio_client).SetEventHandle(event)) {
                    Err(_) => {
                        (*audio_client).Release();
                        panic!("Failed to call SetEventHandle")
                    },
                    Ok(_) => (),
                };

                event
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = {
                let mut max_frames_in_buffer = mem::uninitialized();
                let hresult = (*audio_client).GetBufferSize(&mut max_frames_in_buffer);

                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    Ok(()) => (),
                };

                max_frames_in_buffer
            };

            // Building a `IAudioRenderClient` that will be used to fill the samples buffer.
            let render_client = {
                let mut render_client: *mut winapi::IAudioRenderClient = mem::uninitialized();
                let hresult = (*audio_client).GetService(&winapi::IID_IAudioRenderClient,
                                                         &mut render_client as
                                                             *mut *mut winapi::IAudioRenderClient as
                                                             *mut _);

                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    Ok(()) => (),
                };

                &mut *render_client
            };

            let new_voice_id = VoiceId(self.next_voice_id.fetch_add(1, Ordering::Relaxed));
            assert_ne!(new_voice_id.0, usize::max_value()); // check for overflows

            // Once we built the `VoiceInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            {
                let inner = VoiceInner {
                    id: new_voice_id.clone(),
                    audio_client: audio_client,
                    render_client: render_client,
                    event: event,
                    playing: false,
                    max_frames_in_buffer: max_frames_in_buffer,
                    bytes_per_frame: format.nBlockAlign,
                };

                self.commands.lock().unwrap().push(Command::NewVoice(inner));

                let result = kernel32::SetEvent(self.pending_scheduled_event);
                assert!(result != 0);
            };

            Ok(new_voice_id)
        }
    }

    #[inline]
    pub fn destroy_voice(&self, voice_id: VoiceId) {
        unsafe {
            self.commands
                .lock()
                .unwrap()
                .push(Command::DestroyVoice(voice_id));
            let result = kernel32::SetEvent(self.pending_scheduled_event);
            assert!(result != 0);
        }
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(VoiceId, UnknownTypeBuffer)
    {
        self.run_inner(&mut callback);
    }

    fn run_inner(&self, callback: &mut FnMut(VoiceId, UnknownTypeBuffer)) -> ! {
        unsafe {
            // We keep `run_context` locked forever, which guarantees that two invocations of
            // `run()` cannot run simultaneously.
            let mut run_context = self.run_context.lock().unwrap();

            loop {
                // Process the pending commands.
                let mut commands_lock = self.commands.lock().unwrap();
                for command in commands_lock.drain(..) {
                    match command {
                        Command::NewVoice(voice_inner) => {
                            let event = voice_inner.event;
                            run_context.voices.push(voice_inner);
                            run_context.handles.push(event);
                        },
                        Command::DestroyVoice(voice_id) => {
                            match run_context.voices.iter().position(|v| v.id == voice_id) {
                                None => continue,
                                Some(p) => {
                                    run_context.handles.remove(p + 1);
                                    run_context.voices.remove(p);
                                },
                            }
                        },
                        Command::Play(voice_id) => {
                            if let Some(v) = run_context.voices.get_mut(voice_id.0) {
                                if !v.playing {
                                    let hresult = (*v.audio_client).Start();
                                    check_result(hresult).unwrap();
                                    v.playing = true;
                                }
                            }
                        },
                        Command::Pause(voice_id) => {
                            if let Some(v) = run_context.voices.get_mut(voice_id.0) {
                                if v.playing {
                                    let hresult = (*v.audio_client).Stop();
                                    check_result(hresult).unwrap();
                                    v.playing = true;
                                }
                            }
                        },
                    }
                }
                drop(commands_lock);

                // Wait for any of the handles to be signalled, which means that the corresponding
                // sound needs a buffer.
                debug_assert!(run_context.handles.len() <= winapi::MAXIMUM_WAIT_OBJECTS as usize);
                let result = kernel32::WaitForMultipleObjectsEx(run_context.handles.len() as u32,
                                                                run_context.handles.as_ptr(),
                                                                winapi::FALSE,
                                                                winapi::INFINITE, /* TODO: allow setting a timeout */
                                                                winapi::FALSE /* irrelevant parameter here */);

                // Notifying the corresponding task handler.
                debug_assert!(result >= winapi::WAIT_OBJECT_0);
                let handle_id = (result - winapi::WAIT_OBJECT_0) as usize;

                // If `handle_id` is 0, then it's `pending_scheduled_event` that was signalled in
                // order for us to pick up the pending commands.
                // Otherwise, a voice needs data.
                if handle_id >= 1 {
                    let voice = &mut run_context.voices[handle_id - 1];
                    let voice_id = voice.id.clone();

                    // Obtaining the number of frames that are available to be written.
                    let frames_available = {
                        let mut padding = mem::uninitialized();
                        let hresult = (*voice.audio_client).GetCurrentPadding(&mut padding);
                        check_result(hresult).unwrap();
                        voice.max_frames_in_buffer - padding
                    };

                    if frames_available == 0 {
                        // TODO: can this happen?
                        continue;
                    }

                    // Obtaining a pointer to the buffer.
                    let (buffer_data, buffer_len) = {
                        let mut buffer: *mut winapi::BYTE = mem::uninitialized();
                        let hresult = (*voice.render_client)
                            .GetBuffer(frames_available, &mut buffer as *mut *mut _);
                        check_result(hresult).unwrap(); // FIXME: can return `AUDCLNT_E_DEVICE_INVALIDATED`
                        debug_assert!(!buffer.is_null());

                        (buffer as *mut _,
                         frames_available as usize * voice.bytes_per_frame as usize /
                             mem::size_of::<f32>()) // FIXME: correct size when not f32
                    };

                    let buffer = Buffer {
                        voice: voice,
                        buffer_data: buffer_data,
                        buffer_len: buffer_len,
                        frames: frames_available,
                        marker: PhantomData,
                    };

                    let buffer = UnknownTypeBuffer::F32(::Buffer { target: Some(buffer) }); // FIXME: not always f32
                    callback(voice_id, buffer);
                }
            }
        }
    }

    #[inline]
    pub fn play(&self, voice: VoiceId) {
        unsafe {
            self.commands.lock().unwrap().push(Command::Play(voice));
            let result = kernel32::SetEvent(self.pending_scheduled_event);
            assert!(result != 0);
        }
    }

    #[inline]
    pub fn pause(&self, voice: VoiceId) {
        unsafe {
            self.commands.lock().unwrap().push(Command::Pause(voice));
            let result = kernel32::SetEvent(self.pending_scheduled_event);
            assert!(result != 0);
        }
    }
}

impl Drop for EventLoop {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            kernel32::CloseHandle(self.pending_scheduled_event);
        }
    }
}

unsafe impl Send for EventLoop {
}
unsafe impl Sync for EventLoop {
}

// The content of a voice ID is a number that was fetched from `next_voice_id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoiceId(usize);

impl Drop for VoiceInner {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.render_client).Release();
            (*self.audio_client).Release();
            kernel32::CloseHandle(self.event);
        }
    }
}

pub struct Buffer<'a, T: 'a> {
    voice: &'a mut VoiceInner,

    buffer_data: *mut T,
    buffer_len: usize,
    frames: winapi::UINT32,

    marker: PhantomData<&'a mut [T]>,
}

unsafe impl<'a, T> Send for Buffer<'a, T> {
}

impl<'a, T> Buffer<'a, T> {
    #[inline]
    pub fn buffer(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.buffer_data, self.buffer_len) }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer_len
    }

    #[inline]
    pub fn finish(self) {
        unsafe {
            let hresult = (*self.voice.render_client).ReleaseBuffer(self.frames as u32, 0);
            match check_result(hresult) {
                // Ignoring the error that is produced if the device has been disconnected.
                Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => (),
                e => e.unwrap(),
            };
        }
    }
}

// Turns a `Format` into a `WAVEFORMATEXTENSIBLE`.
fn format_to_waveformatextensible(format: &Format)
                                  -> Result<winapi::WAVEFORMATEXTENSIBLE, CreationError> {
    Ok(winapi::WAVEFORMATEXTENSIBLE {
           Format: winapi::WAVEFORMATEX {
               wFormatTag: match format.data_type {
                   SampleFormat::I16 => winapi::WAVE_FORMAT_PCM,
                   SampleFormat::F32 => winapi::WAVE_FORMAT_EXTENSIBLE,
                   SampleFormat::U16 => return Err(CreationError::FormatNotSupported),
               },
               nChannels: format.channels as winapi::WORD,
               nSamplesPerSec: format.sample_rate.0 as winapi::DWORD,
               nAvgBytesPerSec: format.channels as winapi::DWORD *
                   format.sample_rate.0 as winapi::DWORD *
                   format.data_type.sample_size() as winapi::DWORD,
               nBlockAlign: format.channels as winapi::WORD *
                   format.data_type.sample_size() as winapi::WORD,
               wBitsPerSample: 8 * format.data_type.sample_size() as winapi::WORD,
               cbSize: match format.data_type {
                   SampleFormat::I16 => 0,
                   SampleFormat::F32 => (mem::size_of::<winapi::WAVEFORMATEXTENSIBLE>() -
                                             mem::size_of::<winapi::WAVEFORMATEX>()) as
                       winapi::WORD,
                   SampleFormat::U16 => return Err(CreationError::FormatNotSupported),
               },
           },
           Samples: 8 * format.data_type.sample_size() as winapi::WORD,
           dwChannelMask: {
               let mut mask = 0;

               const CHANNEL_POSITIONS: &'static [winapi::DWORD] = &[
                    winapi::SPEAKER_FRONT_LEFT,
                    winapi::SPEAKER_FRONT_RIGHT,
                    winapi::SPEAKER_FRONT_CENTER,
                    winapi::SPEAKER_LOW_FREQUENCY,
                    winapi::SPEAKER_BACK_LEFT,
                    winapi::SPEAKER_BACK_RIGHT,
                    winapi::SPEAKER_FRONT_LEFT_OF_CENTER,
                    winapi::SPEAKER_FRONT_RIGHT_OF_CENTER,
                    winapi::SPEAKER_BACK_CENTER,
                    winapi::SPEAKER_SIDE_LEFT,
                    winapi::SPEAKER_SIDE_RIGHT,
                    winapi::SPEAKER_TOP_CENTER,
                    winapi::SPEAKER_TOP_FRONT_LEFT,
                    winapi::SPEAKER_TOP_FRONT_CENTER,
                    winapi::SPEAKER_TOP_FRONT_RIGHT,
                    winapi::SPEAKER_TOP_BACK_LEFT,
                    winapi::SPEAKER_TOP_BACK_CENTER,
                    winapi::SPEAKER_TOP_BACK_RIGHT,
               ];

               for i in 0..format.channels {
                   let raw_value = CHANNEL_POSITIONS[i as usize];
                   mask = mask | raw_value;
               }

               mask
           },
           SubFormat: match format.data_type {
               SampleFormat::I16 => winapi::KSDATAFORMAT_SUBTYPE_PCM,
               SampleFormat::F32 => winapi::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
               SampleFormat::U16 => return Err(CreationError::FormatNotSupported),
           },
       })
}

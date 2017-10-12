
use super::Endpoint;
use super::check_result;
use super::com;
use super::kernel32;
use super::ole32;
use super::winapi;

use std::iter;
use std::marker::PhantomData;
use std::mem;
use std::ptr;
use std::slice;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use ChannelPosition;
use CreationError;
use Format;
use SampleFormat;
use UnknownTypeBuffer;

pub struct EventLoop {
    inner: Arc<EventLoopInner>,
}

struct EventLoopInner {
    // This event is signalled after new voices have been created or destroyed, so that the `run()`
    // method can be notified.
    pending_scheduled_event: winapi::HANDLE,

    // Voices that have been created in this event loop.
    // The ID of the voice is the index within the `Vec`.
    voices: Mutex<Vec<Option<VoiceInner>>>,
}

struct VoiceInner {
    audio_client: *mut winapi::IAudioClient,
    render_client: *mut winapi::IAudioRenderClient,
    event: winapi::HANDLE,
    playing: bool,
    max_frames_in_buffer: winapi::UINT32,
    bytes_per_frame: winapi::WORD,
}

impl EventLoop {
    pub fn new() -> EventLoop {
        let pending_scheduled_event =
            unsafe { kernel32::CreateEventA(ptr::null_mut(), 0, 0, ptr::null()) };

        EventLoop {
            inner: Arc::new(EventLoopInner {
                                pending_scheduled_event: pending_scheduled_event,
                                voices: Mutex::new(Vec::new()),
                            }),
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

            // Everything went fine. Adding the voice to the list of voices.
            let voice_id = {
                let inner = VoiceInner {
                                audio_client: audio_client,
                                render_client: render_client,
                                event: event,
                                playing: false,
                                max_frames_in_buffer: max_frames_in_buffer,
                                bytes_per_frame: format.nBlockAlign,
                            };

                let mut voices_lock = self.inner.voices.lock().unwrap();
                if let Some(id) = voices_lock.iter().position(|n| n.is_none()) {
                    voices_lock[id] = Some(inner);
                    id
                } else {
                    let len = voices_lock.len();
                    voices_lock.push(Some(inner));
                    len
                }
            };

            // We signal the event, so that if `run()` is running, it will pick up the changes.
            let result = kernel32::SetEvent(self.inner.pending_scheduled_event);
            assert!(result != 0);

            Ok(VoiceId(voice_id))
        }
    }

    #[inline]
    pub fn destroy_voice(&self, voice_id: VoiceId) {
        unimplemented!()
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(VoiceId, UnknownTypeBuffer)
    {
        self.run_inner(&mut callback);
    }

    fn run_inner(&self, callback: &mut FnMut(VoiceId, UnknownTypeBuffer)) -> ! {
        unsafe {
            let mut handles: Vec<winapi::HANDLE> = Vec::new();      // TODO: SmallVec instead
            let mut handles_need_refresh = true;

            loop {
                if handles_need_refresh {
                    let voices_lock = self.inner.voices.lock().unwrap();
                    handles = iter::once(self.inner.pending_scheduled_event).chain(voices_lock.iter().filter_map(|v| v.as_ref()).map(|v| v.event)).collect();
                    handles_need_refresh = false;
                }

                // Wait for any of the handles to be signalled, which means that the corresponding
                // sound needs a buffer.
                debug_assert!(handles.len() <= winapi::MAXIMUM_WAIT_OBJECTS as usize);
                let result = kernel32::WaitForMultipleObjectsEx(handles.len() as u32,
                                                                handles.as_ptr(),
                                                                winapi::FALSE,
                                                                winapi::INFINITE, /* TODO: allow setting a timeout */
                                                                winapi::FALSE /* irrelevant parameter here */);

                // Notifying the corresponding task handler.
                assert!(result >= winapi::WAIT_OBJECT_0);
                let handle_id = (result - winapi::WAIT_OBJECT_0) as usize;

                if handle_id == 0 {
                    // The `pending_scheduled_event` handle has been notified, which means that the
                    // content of `self.voices` has been modified.
                    handles_need_refresh = true;

                } else {
                    let voice_id = VoiceId(handle_id - 1);
                    let mut voices_lock = self.inner.voices.lock().unwrap();
                    let voice = match voices_lock.get_mut(voice_id.0).and_then(|v| v.as_mut()) {
                        Some(v) => v,
                        None => continue,
                    };

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
                            mem::size_of::<f32>()) // FIXME: correct size
                    };

                    let buffer = Buffer {
                        voice: voice,
                        buffer_data: buffer_data,
                        buffer_len: buffer_len,
                        frames: frames_available,
                        marker: PhantomData,
                    };

                    let buffer = UnknownTypeBuffer::F32(::Buffer { target: Some(buffer) });     // FIXME: not always f32
                    callback(voice_id, buffer);
                }
            }
        }
    }

    #[inline]
    pub fn play(&self, voice: VoiceId) {
        let mut voices_lock = self.inner.voices.lock().unwrap();
        let voice = &mut voices_lock[voice.0].as_mut().unwrap();        // TODO: better error

        if !voice.playing {
            unsafe {
                let hresult = (*voice.audio_client).Start();
                check_result(hresult).unwrap();
            }
    
            voice.playing = true;
        }
    }

    #[inline]
    pub fn pause(&self, voice: VoiceId) {
        let mut voices_lock = self.inner.voices.lock().unwrap();
        let voice = &mut voices_lock[voice.0].as_mut().unwrap();        // TODO: better error

        if voice.playing {
            unsafe {
                let hresult = (*voice.audio_client).Stop();
                check_result(hresult).unwrap();
            }
    
            voice.playing = false;
        }
    }
}

impl Drop for EventLoopInner {
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

// The ID of a voice is its index within the `voices` array of the events loop.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoiceId(usize);

impl Drop for VoiceInner {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.render_client).Release();
            (*self.audio_client).Release();
            kernel32::CloseHandle(self.event);      // TODO: no, may be dangling
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
                // ignoring the error that is produced if the device has been disconnected
                Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => (),
                e => e.unwrap(),
            };
        }
    }
}

fn format_to_waveformatextensible(format: &Format)
                                  -> Result<winapi::WAVEFORMATEXTENSIBLE, CreationError> {
    Ok(winapi::WAVEFORMATEXTENSIBLE {
           Format: winapi::WAVEFORMATEX {
               wFormatTag: match format.data_type {
                   SampleFormat::I16 => winapi::WAVE_FORMAT_PCM,
                   SampleFormat::F32 => winapi::WAVE_FORMAT_EXTENSIBLE,
                   SampleFormat::U16 => return Err(CreationError::FormatNotSupported),
               },
               nChannels: format.channels.len() as winapi::WORD,
               nSamplesPerSec: format.samples_rate.0 as winapi::DWORD,
               nAvgBytesPerSec: format.channels.len() as winapi::DWORD *
                   format.samples_rate.0 as winapi::DWORD *
                   format.data_type.get_sample_size() as winapi::DWORD,
               nBlockAlign: format.channels.len() as winapi::WORD *
                   format.data_type.get_sample_size() as winapi::WORD,
               wBitsPerSample: 8 * format.data_type.get_sample_size() as winapi::WORD,
               cbSize: match format.data_type {
                   SampleFormat::I16 => 0,
                   SampleFormat::F32 => (mem::size_of::<winapi::WAVEFORMATEXTENSIBLE>() -
                                             mem::size_of::<winapi::WAVEFORMATEX>()) as
                       winapi::WORD,
                   SampleFormat::U16 => return Err(CreationError::FormatNotSupported),
               },
           },
           Samples: 8 * format.data_type.get_sample_size() as winapi::WORD,
           dwChannelMask: {
               let mut mask = 0;
               for &channel in format.channels.iter() {
                   let raw_value = match channel {
                       ChannelPosition::FrontLeft => winapi::SPEAKER_FRONT_LEFT,
                       ChannelPosition::FrontRight => winapi::SPEAKER_FRONT_RIGHT,
                       ChannelPosition::FrontCenter => winapi::SPEAKER_FRONT_CENTER,
                       ChannelPosition::LowFrequency => winapi::SPEAKER_LOW_FREQUENCY,
                       ChannelPosition::BackLeft => winapi::SPEAKER_BACK_LEFT,
                       ChannelPosition::BackRight => winapi::SPEAKER_BACK_RIGHT,
                       ChannelPosition::FrontLeftOfCenter => winapi::SPEAKER_FRONT_LEFT_OF_CENTER,
                       ChannelPosition::FrontRightOfCenter => winapi::SPEAKER_FRONT_RIGHT_OF_CENTER,
                       ChannelPosition::BackCenter => winapi::SPEAKER_BACK_CENTER,
                       ChannelPosition::SideLeft => winapi::SPEAKER_SIDE_LEFT,
                       ChannelPosition::SideRight => winapi::SPEAKER_SIDE_RIGHT,
                       ChannelPosition::TopCenter => winapi::SPEAKER_TOP_CENTER,
                       ChannelPosition::TopFrontLeft => winapi::SPEAKER_TOP_FRONT_LEFT,
                       ChannelPosition::TopFrontCenter => winapi::SPEAKER_TOP_FRONT_CENTER,
                       ChannelPosition::TopFrontRight => winapi::SPEAKER_TOP_FRONT_RIGHT,
                       ChannelPosition::TopBackLeft => winapi::SPEAKER_TOP_BACK_LEFT,
                       ChannelPosition::TopBackCenter => winapi::SPEAKER_TOP_BACK_CENTER,
                       ChannelPosition::TopBackRight => winapi::SPEAKER_TOP_BACK_RIGHT,
                   };

                   // channels must be in the right order
                   if raw_value <= mask {
                       return Err(CreationError::FormatNotSupported);
                   }

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

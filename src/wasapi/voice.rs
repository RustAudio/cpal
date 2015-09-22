use super::com;
use super::ole32;
use super::winapi;
use super::Endpoint;
use super::check_result;

use std::cmp;
use std::slice;
use std::mem;
use std::ptr;
use std::marker::PhantomData;

use CreationError;
use ChannelPosition;
use Format;
use SampleFormat;

pub struct Voice {
    audio_client: *mut winapi::IAudioClient,
    render_client: *mut winapi::IAudioRenderClient,
    max_frames_in_buffer: winapi::UINT32,
    bytes_per_frame: winapi::WORD,
    playing: bool,
}

unsafe impl Send for Voice {}
unsafe impl Sync for Voice {}

impl Voice {
    pub fn new(end_point: &Endpoint, format: &Format) -> Result<Voice, CreationError> {
        unsafe {
            // making sure that COM is initialized
            // it's not actually sure that this is required, but when in doubt do it
            com::com_initialized();

            // obtaining a `IAudioClient`
            let audio_client = match end_point.build_audioclient() {
                Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    return Err(CreationError::DeviceNotAvailable),
                e => e.unwrap(),
            };

            // computing the format and initializing the device
            let format = {
                let format_attempt = try!(format_to_waveformatextensible(format));
                let share_mode = winapi::AUDCLNT_SHAREMODE::AUDCLNT_SHAREMODE_SHARED;

                // `IsFormatSupported` checks whether the format is supported and fills
                // a `WAVEFORMATEX`
                let mut dummy_fmt_ptr: *mut winapi::WAVEFORMATEX = mem::uninitialized();
                let hresult = (*audio_client).IsFormatSupported(share_mode, &format_attempt.Format,
                                                                &mut dummy_fmt_ptr);
                // we free that `WAVEFORMATEX` immediately after because we don't need it
                if !dummy_fmt_ptr.is_null() {
                    ole32::CoTaskMemFree(dummy_fmt_ptr as *mut _);
                }

                // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
                // has been found) but we also treat this as an error
                match (hresult, check_result(hresult)) {
                    (_, Err(ref e))
                            if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    {
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
                let hresult = (*audio_client).Initialize(share_mode, 0, 10000000, 0,
                                                         &format_attempt.Format, ptr::null());
                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    {
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

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = {
                let mut max_frames_in_buffer = mem::uninitialized();
                let hresult = (*audio_client).GetBufferSize(&mut max_frames_in_buffer);

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    {
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

            // building a `IAudioRenderClient` that will be used to fill the samples buffer
            let render_client = {
                let mut render_client: *mut winapi::IAudioRenderClient = mem::uninitialized();
                let hresult = (*audio_client).GetService(&winapi::IID_IAudioRenderClient,
                                                         &mut render_client
                                                            as *mut *mut winapi::IAudioRenderClient
                                                            as *mut _);

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    {
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

            // everything went fine
            Ok(Voice {
                audio_client: audio_client,
                render_client: render_client,
                max_frames_in_buffer: max_frames_in_buffer,
                bytes_per_frame: format.nBlockAlign,
                playing: false,
            })
        }
    }

    pub fn append_data<'a, T>(&'a mut self, max_elements: usize) -> Buffer<'a, T> {
        unsafe {
            // obtaining the number of frames that are available to be written
            let frames_available = {
                let mut padding = mem::uninitialized();
                let hresult = (*self.audio_client).GetCurrentPadding(&mut padding);
                check_result(hresult).unwrap();
                self.max_frames_in_buffer - padding
            };

            // making sure `frames_available` is inferior to `max_elements`
            let frames_available = cmp::min(frames_available,
                                            max_elements as u32 * mem::size_of::<T>() as u32 /
                                            self.bytes_per_frame as u32);

            // the WASAPI has some weird behaviors when the buffer size is zero, so we handle this
            // ourselves
            if frames_available == 0 {
                return Buffer::Empty;
            }

            // obtaining a pointer to the buffer
            let (buffer_data, buffer_len) = {
                let mut buffer: *mut winapi::BYTE = mem::uninitialized();
                let hresult = (*self.render_client).GetBuffer(frames_available,
                                                              &mut buffer as *mut *mut _);
                check_result(hresult).unwrap();     // FIXME: can return `AUDCLNT_E_DEVICE_INVALIDATED`
                debug_assert!(!buffer.is_null());

                (buffer as *mut T,
                 frames_available as usize * self.bytes_per_frame as usize / mem::size_of::<T>())
            };

            Buffer::Buffer {
                render_client: self.render_client,
                buffer_data: buffer_data,
                buffer_len: buffer_len,
                frames: frames_available,
                marker: PhantomData,
            }
        }
    }

    #[inline]
    pub fn play(&mut self) {
        if !self.playing {
            unsafe {
                let hresult = (*self.audio_client).Start();
                check_result(hresult).unwrap();
            }
        }

        self.playing = true;
    }

    #[inline]
    pub fn pause(&mut self) {
        if self.playing {
            unsafe {
                let hresult = (*self.audio_client).Stop();
                check_result(hresult).unwrap();
            }
        }

        self.playing = false;
    }

    pub fn underflowed(&self) -> bool {
        unsafe {
            let mut padding = mem::uninitialized();
            let hresult = (*self.audio_client).GetCurrentPadding(&mut padding);
            check_result(hresult).unwrap();
            
            padding == 0
        }
    }
}

impl Drop for Voice {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.render_client).Release();
            (*self.audio_client).Release();
        }
    }
}

pub enum Buffer<'a, T: 'a> {
    Empty,
    Buffer {
        render_client: *mut winapi::IAudioRenderClient,
        buffer_data: *mut T,
        buffer_len: usize,
        frames: winapi::UINT32,
        marker: PhantomData<&'a mut T>,
    },
}

impl<'a, T> Buffer<'a, T> {
    #[inline]
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        match self {
            &mut Buffer::Empty => &mut [],
            &mut Buffer::Buffer { buffer_data, buffer_len, .. } => unsafe {
                slice::from_raw_parts_mut(buffer_data, buffer_len)
            },
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match self {
            &Buffer::Empty => 0,
            &Buffer::Buffer { buffer_len, .. } => buffer_len,
        }
    }

    #[inline]
    pub fn finish(self) {
        if let Buffer::Buffer { render_client, frames, .. } = self {
            unsafe {
                let hresult = (*render_client).ReleaseBuffer(frames as u32, 0);
                match check_result(hresult) {
                    // ignoring the error that is produced if the device has been disconnected
                    Err(ref e)
                            if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => (),
                    e => e.unwrap(),
                };
            }
        }
    }
}

fn format_to_waveformatextensible(format: &Format)
                                  -> Result<winapi::WAVEFORMATEXTENSIBLE, CreationError>
{
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
                                      mem::size_of::<winapi::WAVEFORMATEX>()) as winapi::WORD,
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

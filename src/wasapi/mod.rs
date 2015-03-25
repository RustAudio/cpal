extern crate libc;
extern crate winapi;
extern crate "ole32-sys" as ole32;

use std::{slice, mem, ptr};
use std::marker::PhantomData;

// TODO: determine if should be NoSend or not
pub struct Voice {
    audio_client: *mut winapi::IAudioClient,
    render_client: *mut winapi::IAudioRenderClient,
    max_frames_in_buffer: winapi::UINT32,
    num_channels: winapi::WORD,
    bytes_per_frame: winapi::WORD,
    samples_per_second: winapi::DWORD,
    bits_per_sample: winapi::WORD,
    playing: bool,
}

pub struct Buffer<'a, T: 'a> {
    render_client: *mut winapi::IAudioRenderClient,
    buffer_data: *mut T,
    buffer_len: usize,
    frames: winapi::UINT32,
    marker: PhantomData<&'a mut T>,
}

impl Voice {
    pub fn new() -> Voice {
        init().unwrap()
    }

    pub fn get_channels(&self) -> ::ChannelsCount {
        self.num_channels as ::ChannelsCount
    }

    pub fn get_samples_rate(&self) -> ::SamplesRate {
        ::SamplesRate(self.samples_per_second as u32)
    }

    pub fn get_samples_format(&self) -> ::SampleFormat {
        match self.bits_per_sample {
            16 => ::SampleFormat::U16,
            _ => unimplemented!(),
        }
    }

    pub fn append_data<'a, T>(&'a mut self, max_elements: usize) -> Buffer<'a, T> {
        unsafe {
            loop {
                // 
                let frames_available = {
                    let mut padding = mem::uninitialized();
                    let f = self.audio_client.as_mut().unwrap().lpVtbl
                                .as_ref().unwrap().GetCurrentPadding;
                    let hresult = f(self.audio_client, &mut padding);
                    check_result(hresult).unwrap();
                    self.max_frames_in_buffer - padding
                };

                if frames_available == 0 {
                    // TODO: 
                    ::std::thread::sleep(::std::time::duration::Duration::milliseconds(1));
                    continue;
                }

                let frames_available = ::std::cmp::min(frames_available,
                                                       max_elements as u32 * mem::size_of::<T>() as u32 /
                                                       self.bytes_per_frame as u32);
                assert!(frames_available != 0);

                // loading buffer
                let (buffer_data, buffer_len) = {
                    let mut buffer: *mut winapi::BYTE = mem::uninitialized();
                    let f = self.render_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().GetBuffer;
                    let hresult = f(self.render_client, frames_available,
                                    &mut buffer as *mut *mut libc::c_uchar);
                    check_result(hresult).unwrap();
                    assert!(!buffer.is_null());

                    (buffer as *mut T,
                     frames_available as usize * self.bytes_per_frame as usize
                          / mem::size_of::<T>())
                };

                let buffer = Buffer {
                    render_client: self.render_client,
                    buffer_data: buffer_data,
                    buffer_len: buffer_len,
                    frames: frames_available,
                    marker: PhantomData,
                };

                return buffer;
            }
        }
    }

    pub fn play(&mut self) {
        if !self.playing {
            unsafe {
                let f = self.audio_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().Start;
                let hresult = f(self.audio_client);
                check_result(hresult).unwrap();
            }
        }

        self.playing = true;
    }

    pub fn pause(&mut self) {
        if self.playing {
            unsafe {
                let f = self.audio_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().Stop;
                let hresult = f(self.audio_client);
                check_result(hresult).unwrap();
            }
        }

        self.playing = false;
    }
}

unsafe impl Send for Voice {}
unsafe impl Sync for Voice {}

impl Drop for Voice {
    fn drop(&mut self) {
        unsafe {
            {
                let f = self.render_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().Release;
                f(self.render_client);
            }

            {
                let f = self.audio_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().Release;
                f(self.audio_client);
            }
        }
    }
}

impl<'a, T> Buffer<'a, T> {
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        unsafe {
            slice::from_raw_parts_mut(self.buffer_data, self.buffer_len)
        }
    }

    pub fn finish(self) {
        // releasing buffer
        unsafe {
            let f = self.render_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().ReleaseBuffer;
            let hresult = f(self.render_client, self.frames as u32, 0);
            check_result(hresult).unwrap();
        };
    }
}

fn init() -> Result<Voice, String> {
    // FIXME: release everything
    unsafe {
        try!(check_result(ole32::CoInitializeEx(::std::ptr::null_mut(), 0)));

        // building the devices enumerator object
        let enumerator = {
            let mut enumerator: *mut winapi::IMMDeviceEnumerator = ::std::mem::uninitialized();
            
            let hresult = ole32::CoCreateInstance(&winapi::CLSID_MMDeviceEnumerator,
                                                   ptr::null_mut(), winapi::CLSCTX_ALL,
                                                   &winapi::IID_IMMDeviceEnumerator,
                                                   mem::transmute(&mut enumerator));

            try!(check_result(hresult));
            enumerator.as_mut().unwrap()
        };

        // getting the default end-point
        let device = {
            let mut device: *mut winapi::IMMDevice = mem::uninitialized();
            let f = enumerator.lpVtbl.as_ref().unwrap().GetDefaultAudioEndpoint;
            let hresult = f(enumerator, winapi::EDataFlow::eRender, winapi::ERole::eConsole,
                            mem::transmute(&mut device));
            try!(check_result(hresult));
            device.as_mut().unwrap()
        };

        // activating in order to get a `IAudioClient`
        let audio_client = {
            let mut audio_client: *mut winapi::IAudioClient = mem::uninitialized();
            let f = device.lpVtbl.as_ref().unwrap().Activate;
            let hresult = f(device, &winapi::IID_IAudioClient, winapi::CLSCTX_ALL,
                            ptr::null_mut(), mem::transmute(&mut audio_client));
            try!(check_result(hresult));
            audio_client.as_mut().unwrap()
        };

        // computing the format and initializing the device
        let format = {
            let format_attempt = winapi::WAVEFORMATEX {
                wFormatTag: 1,      // WAVE_FORMAT_PCM ; TODO: replace by constant
                nChannels: 2,
                nSamplesPerSec: 44100,
                nAvgBytesPerSec: 2 * 44100 * 2,
                nBlockAlign: (2 * 16) / 8,
                wBitsPerSample: 16,
                cbSize: 0,
            };

            let mut format_ptr: *mut winapi::WAVEFORMATEX = mem::uninitialized();
            let f = audio_client.lpVtbl.as_ref().unwrap().IsFormatSupported;
            let hresult = f(audio_client, winapi::AUDCLNT_SHAREMODE::AUDCLNT_SHAREMODE_SHARED,
                            &format_attempt, &mut format_ptr);
            try!(check_result(hresult));

            let format = match format_ptr.as_ref() {
                Some(f) => f,
                None => &format_attempt,
            };

            let format_copy = ptr::read(format);

            let f = audio_client.lpVtbl.as_ref().unwrap().Initialize;
            let hresult = f(audio_client, winapi::AUDCLNT_SHAREMODE::AUDCLNT_SHAREMODE_SHARED,
                            0, 10000000, 0, format, ptr::null());

            if !format_ptr.is_null() {
                ole32::CoTaskMemFree(format_ptr as *mut libc::c_void);
            }

            try!(check_result(hresult));

            format_copy
        };

        // 
        let max_frames_in_buffer = {
            let mut max_frames_in_buffer = mem::uninitialized();
            let f = audio_client.lpVtbl.as_ref().unwrap().GetBufferSize;
            let hresult = f(audio_client, &mut max_frames_in_buffer);
            try!(check_result(hresult));
            max_frames_in_buffer
        };

        // 
        let render_client = {
            let mut render_client: *mut winapi::IAudioRenderClient = mem::uninitialized();
            let f = audio_client.lpVtbl.as_ref().unwrap().GetService;
            let hresult = f(audio_client, &winapi::IID_IAudioRenderClient,
                            mem::transmute(&mut render_client));
            try!(check_result(hresult));
            render_client.as_mut().unwrap()
        };

        Ok(Voice {
            audio_client: audio_client,
            render_client: render_client,
            max_frames_in_buffer: max_frames_in_buffer,
            num_channels: format.nChannels,
            bytes_per_frame: format.nBlockAlign,
            samples_per_second: format.nSamplesPerSec,
            bits_per_sample: format.wBitsPerSample,
            playing: false,
        })
    }
}

fn check_result(result: winapi::HRESULT) -> Result<(), String> {
    if result < 0 {
        return Err(format!("Error in winapi call"));        // TODO: 
    }

    Ok(())
}

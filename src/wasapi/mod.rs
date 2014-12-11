extern crate libc;
extern crate winapi;

use std::{mem, ptr};
use std::c_vec::CVec;

pub struct Channel {
    audio_client: *mut winapi::IAudioClient,
    render_client: *mut winapi::IAudioRenderClient,
    max_frames_in_buffer: winapi::UINT32,
    num_channels: winapi::WORD,
    bytes_per_frame: winapi::WORD,
    samples_per_second: winapi::DWORD,
    bits_per_sample: winapi::WORD,
    started: bool,
}

pub struct Buffer<'a> {
    audio_client: *mut winapi::IAudioClient,
    render_client: *mut winapi::IAudioRenderClient,
    buffer: CVec<u8>,
    frames: winapi::UINT32,
    start_on_drop: bool,
}

impl Channel {
    pub fn new() -> Channel {
        init().unwrap()
    }

    pub fn get_channels(&self) -> ::ChannelsCount {
        self.num_channels as ::ChannelsCount
    }

    pub fn get_samples_per_second(&self) -> u32 {
        self.samples_per_second as u32
    }

    pub fn get_samples_format(&self) -> ::SampleFormat {
        match self.bits_per_sample {
            16 => ::SampleFormat::U16,
            _ => unimplemented!(),
        }
    }

    pub fn append_data<'a>(&'a mut self) -> Buffer<'a> {
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
                    ::std::io::timer::sleep(::std::time::duration::Duration::milliseconds(5));
                    continue;
                }

                // loading buffer
                let buffer: CVec<u8> = {
                    let mut buffer: *mut winapi::BYTE = mem::uninitialized();
                    let f = self.render_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().GetBuffer;
                    let hresult = f(self.render_client, frames_available,
                                    &mut buffer as *mut *mut libc::c_uchar);
                    check_result(hresult).unwrap();
                    assert!(!buffer.is_null());

                    CVec::new(buffer as *mut u8,
                              frames_available as uint * self.bytes_per_frame as uint)
                };

                let buffer = Buffer {
                    audio_client: self.audio_client,
                    render_client: self.render_client,
                    buffer: buffer,
                    frames: frames_available,
                    start_on_drop: !self.started,
                };

                self.started = true;
                return buffer;
            }
        }
    }
}

impl<'a> Buffer<'a> {
    pub fn get_buffer(&mut self) -> &mut [u8] {
        self.buffer.as_mut_slice()
    }
}

#[unsafe_destructor]
impl<'a> Drop for Buffer<'a> {
    fn drop(&mut self) {
        // releasing buffer
        unsafe {
            let f = self.render_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().ReleaseBuffer;
            let hresult = f(self.render_client, self.frames, 0);
            check_result(hresult).unwrap();

            if self.start_on_drop {
                let f = self.audio_client.as_mut().unwrap().lpVtbl.as_ref().unwrap().Start;
                let hresult = f(self.audio_client);
                check_result(hresult).unwrap();
            }
        };
    }
}

fn init() -> Result<Channel, String> {
    // FIXME: release everything
    unsafe {
        try!(check_result(winapi::CoInitializeEx(::std::ptr::null_mut(), 0)));

        // building the devices enumerator object
        let enumerator = {
            let mut enumerator: *mut winapi::IMMDeviceEnumerator = ::std::mem::uninitialized();
            
            let hresult = winapi::CoCreateInstance(&winapi::CLSID_MMDeviceEnumerator,
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

            let format_copy = *format;

            let f = audio_client.lpVtbl.as_ref().unwrap().Initialize;
            let hresult = f(audio_client, winapi::AUDCLNT_SHAREMODE::AUDCLNT_SHAREMODE_SHARED,
                            0, 10000000, 0, format, ptr::null());

            if !format_ptr.is_null() {
                winapi::CoTaskMemFree(format_ptr as *mut libc::c_void);
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

        Ok(Channel {
            audio_client: audio_client,
            render_client: render_client,
            max_frames_in_buffer: max_frames_in_buffer,
            num_channels: format.nChannels,
            bytes_per_frame: format.nBlockAlign,
            samples_per_second: format.nSamplesPerSec,
            bits_per_sample: format.wBitsPerSample,
            started: false,
        })
    }
}
/*
        let mut started = false;
        loop {
            // 
            let frames_available = if started {
                let mut padding = mem::uninitialized();
                let f = audio_client.lpVtbl.as_ref().unwrap().GetCurrentPadding;
                let hresult = f(audio_client, &mut padding);
                try!(check_result(hresult));
                buffer_frame_count - padding
            } else {
                buffer_frame_count
            };

            if frames_available == 0 {
                ::std::io::timer::sleep(::std::time::duration::Duration::milliseconds((1000.0 * (44100 - frames_available) as f32 / 44100.0) as i64));
                continue;
            }

            // loading buffer
            let mut buffer: CVec<u16> = {
                let mut buffer: *mut winapi::BYTE = mem::uninitialized();
                let f = render_client.lpVtbl.as_ref().unwrap().GetBuffer;
                let hresult = f(render_client, frames_available,
                                &mut buffer as *mut *mut libc::c_uchar);
                try!(check_result(hresult));
                assert!(!buffer.is_null());
                CVec::new(buffer as *mut u16, frames_available as uint)     // TODO: size of a frame?
            };

            // generating sinosoïd
            let mut angle = 0.0f32;
            for elem in buffer.as_mut_slice().iter_mut() {
                use std::num::Int;
                use std::num::FloatMath;

                angle += 1.0;
                let value = angle.sin();
                let max: u16 = Int::max_value();
                let value = (value * max as f32) as u16;
                *elem = value;
            }

            // releasing buffer
            {
                let f = render_client.lpVtbl.as_ref().unwrap().ReleaseBuffer;
                let hresult = f(render_client, frames_available, 0);
                try!(check_result(hresult));
            };

            //
            if !started {
                let f = audio_client.lpVtbl.as_ref().unwrap().Start;
                let hresult = f(audio_client);
                try!(check_result(hresult));
                started = true;
            }
        }
    };

    Ok(())
}*/

fn check_result(result: winapi::HRESULT) -> Result<(), String> {
    if result < 0 {
        return Err(::std::os::error_string(result as uint));        // TODO: 
    }

    Ok(())
}

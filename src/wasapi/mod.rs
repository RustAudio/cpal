extern crate libc;
extern crate winapi;

use std::{mem, ptr};
use std::c_vec::CVec;

pub fn create() -> Result<(), String> {
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

        // format is a WAVEFORMATEX
        // but we store it as an array of bytes because of the weird format inheritance system
        let format: Vec<u8> = {
            let mut format_ptr: *mut winapi::WAVEFORMATEX = mem::uninitialized();
            let f = audio_client.lpVtbl.as_ref().unwrap().GetMixFormat;
            let hresult = f(audio_client, &mut format_ptr);
            try!(check_result(hresult));
            let format = format_ptr.as_ref().unwrap();
            let mut format_copy = Vec::from_elem(mem::size_of::<winapi::WAVEFORMATEX>() +
                                                 format.cbSize as uint, 0u8);
            ptr::copy_nonoverlapping_memory(format_copy.as_mut_ptr(), mem::transmute(format),
                                            format_copy.len());
            winapi::CoTaskMemFree(format_ptr as *mut libc::c_void);
            format_copy
        };

        // initializing
        {
            let f = audio_client.lpVtbl.as_ref().unwrap().Initialize;
            let hresult = f(audio_client, winapi::AUDCLNT_SHAREMODE::AUDCLNT_SHAREMODE_SHARED, 0, 10000000, 0, format.as_ptr() as *const winapi::WAVEFORMATEX, ptr::null());
            try!(check_result(hresult));
        };

        // 
        let buffer_frame_count = {
            let mut buffer_frame_count = mem::uninitialized();
            let f = audio_client.lpVtbl.as_ref().unwrap().GetBufferSize;
            let hresult = f(audio_client, &mut buffer_frame_count);
            try!(check_result(hresult));
            buffer_frame_count
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
}

fn check_result(result: winapi::HRESULT) -> Result<(), String> {
    if result < 0 {
        return Err(::std::os::error_string(result as uint));        // TODO: 
    }

    Ok(())
}

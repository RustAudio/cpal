extern crate libc;
extern crate winapi;

#[link(name = "uuid")]
extern "C" {
    static CLSID_MMDeviceEnumerator: winapi::CLSID;
    static IID_IMMDeviceEnumerator: winapi::IID;
}

fn create() -> Result<(), String> {
    unsafe {

        try!(check_result(winapi::CoInitializeEx(::std::ptr::null_mut(), 0)));

        let enumerator = {
            let mut enumerator: *mut winapi::IMMDeviceEnumerator = ::std::mem::uninitialized();
            
            let hresult = winapi::CoCreateInstance(&CLSID_MMDeviceEnumerator,
                                                   ::std::ptr::null_mut(), winapi::CLSCTX_ALL,
                                                   &IID_IMMDeviceEnumerator,
                                                   ::std::mem::transmute(&mut enumerator));

            try!(check_result(hresult));
            enumerator.as_mut().unwrap()
        };

        // getting the default end-point
        let device = {
            let mut device: *mut winapi::IMMDevice = ::std::mem::uninitialized();
            let f = enumerator.lpVtbl.as_ref().unwrap().GetDefaultAudioEndpoint;
            let hresult = f(enumerator, winapi::EDataFlow::eRender, winapi::ERole::eConsole,
                            ::std::mem::transmute(&mut device));
            try!(check_result(hresult));
            device.as_mut().unwrap()
        };

        // activating
        let audio_client = {
            //let mut audio_client: *mut winapi::IAudioClient = ::std::mem::uninitialized();
            let f = device.lpVtbl.as_ref().unwrap().Activate;
            //let hresult = f(IID_IAudioClient, winapi::CLSCTX_ALL, ::std::ptr::null_mut(),
            //                ::std::mem::transmute(&mut audio_client));
            //try!(check_result(hresult));
        };
    };

    Ok(())
}

fn check_result(result: winapi::HRESULT) -> Result<(), String> {
    // TODO: 
    Ok(())
}

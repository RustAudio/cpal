mod asio_import;
pub mod errors;

use std::os::raw::c_char;
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_long;
use std::os::raw::c_void;
use std::os::raw::c_double;
use errors::ASIOError;
use std::mem;

use asio_import as ai;

const MAX_DRIVER: usize = 32;

#[derive(Debug)]
pub struct Channel{
    pub ins: i64,
    pub outs: i64,
}

#[derive(Debug)]
pub struct SampleRate{
    pub rate: u32,
}

pub struct AsioStream{
    pub buffer_info: AsioBufferInfo,
    driver: ai::AsioDrivers,
}

#[derive(Debug)]
#[repr(C)]
pub struct AsioBufferInfo{
    is_input: c_long,
    channel_num: c_long,
    buffers: [*mut(); 2],
}

impl AsioStream {
    fn pop_driver(self) -> ai::AsioDrivers{
        self.driver
    }
}

/// Returns the channels for the driver it's passed
///
/// # Arguments
/// * `driver name` - Name of the driver
/// # Usage
/// Use the get_driver_list() to get the list of names
/// Then pass the one you want to get_channels
pub fn get_channels(driver_name: &str) -> Result<Channel, ASIOError>{
    let channel: Result<Channel, ASIOError>;
    // Make owned CString to send to load driver
    let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
    let raw = my_driver_name.into_raw();

    // Initialize memory for calls
    let mut ins: c_long = 0;
    let mut outs: c_long = 0;
    let mut driver_info = ai::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };

    unsafe{
        let mut asio_drivers = ai::AsioDrivers::new();

        let load_result = asio_drivers.loadDriver(raw);

        // Take back ownership
        my_driver_name = CString::from_raw(raw);

        if load_result {
            ai::ASIOInit(&mut driver_info);
            ai::ASIOGetChannels(&mut ins, &mut outs);
            asio_drivers.removeCurrentDriver();
            channel = Ok(Channel{ ins: ins as i64, outs: outs as i64});
        }else{
            channel = Err(ASIOError::NoResult(driver_name.to_owned()));
        }
        ai::destruct_AsioDrivers(&mut asio_drivers);
    }
    
    channel
}

/// Returns a list of all the ASIO drivers
pub fn get_driver_list() -> Vec<String>{
    let mut driver_list: Vec<String> = Vec::new();

    let mut driver_names: [[c_char; MAX_DRIVER]; MAX_DRIVER] = [[0; MAX_DRIVER]; MAX_DRIVER];
    let mut p_driver_name: [*mut i8; MAX_DRIVER] = [0 as *mut i8; MAX_DRIVER];

    for i in 0..MAX_DRIVER{
        p_driver_name[i] = driver_names[i].as_mut_ptr();
    }

    unsafe{

        let mut asio_drivers = ai::AsioDrivers::new();

        let num_drivers = asio_drivers.getDriverNames(p_driver_name.as_mut_ptr(), MAX_DRIVER as i32);

        if num_drivers > 0{
            for i in 0..num_drivers{
                let mut my_driver_name = CString::new("").unwrap();
                let name = CStr::from_ptr(p_driver_name[i as usize]);
                my_driver_name = name.to_owned();
                match my_driver_name.into_string() {
                    Ok(s) => driver_list.push(s),
                    Err(_) => println!("Failed converting from CString"),
                }
            }
        } else {
            println!("No ASIO drivers found");
        }

        ai::destruct_AsioDrivers(&mut asio_drivers);
    }

    driver_list
}


pub fn get_sample_rate(driver_name: &str) -> Result<SampleRate, ASIOError>{
    
    let sample_rate: Result<SampleRate, ASIOError>;
    // Make owned CString to send to load driver
    let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
    let raw = my_driver_name.into_raw();

    // Initialize memory for calls
    let mut rate: c_double = 0.0f64; 
    let mut driver_info = ai::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };

    unsafe{
        let mut asio_drivers = ai::AsioDrivers::new();

        let load_result = asio_drivers.loadDriver(raw);

        // Take back ownership
        my_driver_name = CString::from_raw(raw);

        if load_result {
            ai::ASIOInit(&mut driver_info);
            ai::get_sample_rate(&mut rate);
            asio_drivers.removeCurrentDriver();
            sample_rate = Ok(SampleRate{ rate: rate as u32});
        }else{
            sample_rate = Err(ASIOError::NoResult(driver_name.to_owned()));
        }
        ai::destruct_AsioDrivers(&mut asio_drivers);
    }
    
    sample_rate
}

pub fn prepare_stream(driver_name: &str) -> Result<AsioStream, ASIOError>{
    let mut buffer_info = ai::ASIOBufferInfo{_bindgen_opaque_blob: [0u32; 6]};
    let num_channels = 2;
    let mut callbacks = ai::ASIOCallbacks{_bindgen_opaque_blob: [0u32; 8]};

    let mut min_b_size: c_long = 0;
    let mut max_b_size: c_long = 0;
    let mut pref_b_size: c_long = 0;
    let mut grans: c_long = 0;

    let mut driver_info = ai::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };
    
    // Make owned CString to send to load driver
    let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
    let raw = my_driver_name.into_raw();

    let mut result = Err(ASIOError::NoResult("not implimented".to_owned()));

    unsafe{
        let mut asio_drivers = ai::AsioDrivers::new();
        let load_result = asio_drivers.loadDriver(raw);
        // Take back ownership
        my_driver_name = CString::from_raw(raw);
        if !load_result { return Err(ASIOError::DriverLoadError); }


        ai::ASIOInit(&mut driver_info);
        ai::ASIOGetBufferSize(&mut min_b_size, &mut max_b_size,
                              &mut pref_b_size, &mut grans);
        result = if  pref_b_size > 0 { 
            match ai::ASIOCreateBuffers(&mut buffer_info, num_channels,
                                        pref_b_size, &mut callbacks){
                0 => {
                    Ok(AsioStream{ 
                        buffer_info: mem::transmute::<ai::ASIOBufferInfo,
                        AsioBufferInfo>(buffer_info),
                        driver: asio_drivers
                    })
                },
                _ => Err(ASIOError::BufferError("failed to create buffers".to_owned())),
            }
        }else{
            Err(ASIOError::BufferError("Failed to get buffer size".to_owned()))
        };
    }
    result
}

pub fn destroy_stream(stream: AsioStream) {
    unsafe{
        ai::ASIODisposeBuffers();
        let mut asio_drivers = stream.pop_driver();
        asio_drivers.removeCurrentDriver();
        ai::destruct_AsioDrivers(&mut asio_drivers);
    }
}

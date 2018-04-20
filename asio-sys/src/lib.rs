#[macro_use]
extern crate lazy_static;

extern crate num;
#[macro_use]
extern crate num_derive;

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
use std::sync::Mutex;

use asio_import as ai;

const MAX_DRIVER: usize = 32;

pub struct CbArgs<S, D>{
    pub stream_id: S,
    pub data: D
}

struct BufferCallback(Box<FnMut(i32) + Send>);

lazy_static!{
    static ref buffer_callback: Mutex<Option<BufferCallback>> = Mutex::new(None);
}


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
    pub buffer_infos: [AsioBufferInfo; 2],
    driver: ai::AsioDrivers,
    pub buffer_size: i32,
}

// This is a direct copy of the ASIOSampleType
// inside ASIO SDK. 
#[derive(Debug, FromPrimitive)]
#[repr(C)]
pub enum AsioSampleType{
	ASIOSTInt16MSB   = 0,
	ASIOSTInt24MSB   = 1,		// used for 20 bits as well
	ASIOSTInt32MSB   = 2,
	ASIOSTFloat32MSB = 3,		// IEEE 754 32 bit float
	ASIOSTFloat64MSB = 4,		// IEEE 754 64 bit double float

	// these are used for 32 bit data buffer, with different alignment of the data inside
	// 32 bit PCI bus systems can be more easily used with these
	ASIOSTInt32MSB16 = 8,		// 32 bit data with 16 bit alignment
	ASIOSTInt32MSB18 = 9,		// 32 bit data with 18 bit alignment
	ASIOSTInt32MSB20 = 10,		// 32 bit data with 20 bit alignment
	ASIOSTInt32MSB24 = 11,		// 32 bit data with 24 bit alignment
	
	ASIOSTInt16LSB   = 16,
	ASIOSTInt24LSB   = 17,		// used for 20 bits as well
	ASIOSTInt32LSB   = 18,
	ASIOSTFloat32LSB = 19,		// IEEE 754 32 bit float, as found on Intel x86 architecture
	ASIOSTFloat64LSB = 20, 		// IEEE 754 64 bit double float, as found on Intel x86 architecture

	// these are used for 32 bit data buffer, with different alignment of the data inside
	// 32 bit PCI bus systems can more easily used with these
	ASIOSTInt32LSB16 = 24,		// 32 bit data with 18 bit alignment
	ASIOSTInt32LSB18 = 25,		// 32 bit data with 18 bit alignment
	ASIOSTInt32LSB20 = 26,		// 32 bit data with 20 bit alignment
	ASIOSTInt32LSB24 = 27,		// 32 bit data with 24 bit alignment

	//	ASIO DSD format.
	ASIOSTDSDInt8LSB1   = 32,		// DSD 1 bit data, 8 samples per byte. First sample in Least significant bit.
	ASIOSTDSDInt8MSB1   = 33,		// DSD 1 bit data, 8 samples per byte. First sample in Most significant bit.
	ASIOSTDSDInt8NER8	= 40,		// DSD 8 bit data, 1 sample per byte. No Endianness required.

	ASIOSTLastEntry
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct AsioBufferInfo{
    pub is_input: c_long,
    pub channel_num: c_long,
    pub buffers: [*mut std::os::raw::c_void; 2],
}

#[repr(C)]
struct AsioCallbacks{
    buffer_switch: extern "C" fn(double_buffer_index: c_long,
                                 direct_process: c_long) -> (),
                                 sample_rate_did_change: extern "C" fn(s_rate: c_double) -> (),
                                 asio_message: extern "C" fn(
                                     selector: c_long,
                                     value: c_long,
                                     message: *mut(),
                                     opt: *mut c_double) -> c_long,
                                     buffer_switch_time_info: extern "C" fn(
                                         params: *mut ai::ASIOTime,
                                         double_buffer_index: c_long,
                                         direct_process: c_long) -> *mut ai::ASIOTime,
}

extern "C" fn buffer_switch(double_buffer_index: c_long,
                       direct_process: c_long) -> (){
    
    let mut bc = buffer_callback.lock().unwrap();

    if let Some(ref mut bc) = *bc {
        bc.run(double_buffer_index);
    }
}

extern "C" fn sample_rate_did_change(s_rate: c_double) -> (){
}

extern "C" fn asio_message(selector: c_long,
        value: c_long,
        message: *mut(),
        opt: *mut c_double) -> c_long{
    4 as c_long
}

extern "C" fn buffer_switch_time_info(params: *mut ai::ASIOTime,
                               double_buffer_index: c_long,
                               direct_process: c_long) -> *mut ai::ASIOTime{
    params
}

impl AsioStream {
    fn pop_driver(self) -> ai::AsioDrivers{
        self.driver
    }
}

impl BufferCallback{
    fn run(&mut self, index: i32){
        let mut cb = &mut self.0;
        cb(index);
    }
}

unsafe impl Send for AsioStream{}

pub fn set_callback<F: 'static>(mut callback: F) -> ()
    where F: FnMut(i32) + Send
{
    let mut bc = buffer_callback.lock().unwrap();
    *bc = Some(BufferCallback(Box::new(callback)));
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

pub fn get_data_type(driver_name: &str) -> Result<AsioSampleType, ASIOError>{
    
    let data_type: Result<AsioSampleType, ASIOError>;
    // Make owned CString to send to load driver
    let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
    let raw = my_driver_name.into_raw();

    // Initialize memory for calls
    let mut channel_info = ai::ASIOChannelInfo{
        channel: 0,
        isInput: 0,
        isActive: 0,
        channelGroup: 0,
        type_: 0,
        name: [0 as c_char; 32]
    };
    let mut driver_info = ai::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };

    unsafe{
        let mut asio_drivers = ai::AsioDrivers::new();

        let load_result = asio_drivers.loadDriver(raw);

        // Take back ownership
        my_driver_name = CString::from_raw(raw);

        if load_result {
            ai::ASIOInit(&mut driver_info);
            ai::ASIOGetChannelInfo(&mut channel_info);
            asio_drivers.removeCurrentDriver();
            data_type = num::FromPrimitive::from_i32(channel_info.type_)
                .map_or(Err(ASIOError::TypeError), |t| Ok(t));
        }else{
            data_type = Err(ASIOError::NoResult(driver_name.to_owned()));
        }
        ai::destruct_AsioDrivers(&mut asio_drivers);
    }

    data_type
}

pub fn prepare_stream(driver_name: &str) -> Result<AsioStream, ASIOError>{
    //let mut buffer_info = ai::ASIOBufferInfo{_bindgen_opaque_blob: [0u32; 6]};
    let mut buffer_infos = [AsioBufferInfo{ 
        is_input: 0, 
        channel_num: 0,
        buffers: [std::ptr::null_mut(); 2]
    }, AsioBufferInfo{ 
        is_input: 0, 
        channel_num: 0,
        buffers: [std::ptr::null_mut(); 2]
    }];
    

    let num_channels = 2;
    //let mut callbacks = ai::ASIOCallbacks{_bindgen_opaque_blob: [0u32; 8]};
    let mut callbacks = AsioCallbacks{
        buffer_switch: buffer_switch,
        sample_rate_did_change: sample_rate_did_change,
        asio_message: asio_message,
        buffer_switch_time_info: buffer_switch_time_info
    };

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

        for d in &buffer_infos{
            println!("before {:?}", d);
        }


        ai::ASIOInit(&mut driver_info);
        ai::ASIOGetBufferSize(&mut min_b_size, &mut max_b_size,
                              &mut pref_b_size, &mut grans);
        result = if  pref_b_size > 0 { 
            let mut buffer_info_convert = [
                mem::transmute::<AsioBufferInfo, 
            ai::ASIOBufferInfo>(buffer_infos[0]),
                mem::transmute::<AsioBufferInfo, 
            ai::ASIOBufferInfo>(buffer_infos[1])];
            let mut callbacks_convert = mem::transmute::<AsioCallbacks,
            ai::ASIOCallbacks>(callbacks);
            let buffer_result = ai::ASIOCreateBuffers(buffer_info_convert.as_mut_ptr(), 
                                                             num_channels,
                                                             pref_b_size, 
                                                             &mut callbacks_convert);
            if buffer_result == 0{
                let buffer_infos = [
                    mem::transmute::<ai::ASIOBufferInfo,
                    AsioBufferInfo>(buffer_info_convert[0]),
                    mem::transmute::<ai::ASIOBufferInfo,
                    AsioBufferInfo>(buffer_info_convert[1])];
                for d in &buffer_infos{
                    println!("after {:?}", d);
                }
                println!("channels: {:?}", num_channels);

                return Ok(AsioStream{ 
                    buffer_infos: buffer_infos,
                    driver: asio_drivers,
                    buffer_size: pref_b_size,
                });
            }
            Err(ASIOError::BufferError(format!("failed to create buffers, 
                                       error code: {}", buffer_result)))
        }else{
            Err(ASIOError::BufferError("Failed to get buffer size".to_owned()))
        };

        asio_drivers.removeCurrentDriver();
        ai::destruct_AsioDrivers(&mut asio_drivers);
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


pub fn play(){
    unsafe{
        let result = ai::ASIOStart();
        println!("start result: {}", result);
    }
}

pub fn stop(){
    unsafe{
        let result = ai::ASIOStop();
        println!("start result: {}", result);
    }
}

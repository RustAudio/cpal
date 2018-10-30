#![allow(non_camel_case_types)]

#[macro_use]
extern crate lazy_static;

extern crate num;
#[macro_use]
extern crate num_derive;

mod asio_import;
#[macro_use]
pub mod errors;

use errors::{AsioError, AsioDriverError, AsioErrorWrapper};
use std::ffi::CStr;
use std::ffi::CString;
use std::mem;
use std::os::raw::c_char;
use std::os::raw::c_double;
use std::os::raw::c_long;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::MutexGuard;

use asio_import as ai;

const MAX_DRIVER: usize = 32;

pub struct CbArgs<S, D> {
    pub stream_id: S,
    pub data: D,
}

struct BufferCallback(Box<FnMut(i32) + Send>);

lazy_static! {
    static ref buffer_callback: Mutex<Vec<Option<BufferCallback>>> = Mutex::new(Vec::new());
}

lazy_static! {
    static ref ASIO_DRIVERS: Mutex<DriverWrapper> = Mutex::new(DriverWrapper{
        drivers: None,
        state: AsioState::Offline,
    });
}

static STREAM_DRIVER_COUNT: AtomicUsize = AtomicUsize::new(0);
pub static SILENCE_FIRST: AtomicBool = AtomicBool::new(false);
pub static SILENCE_SECOND: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub struct Channel {
    pub ins: i64,
    pub outs: i64,
}

#[derive(Debug)]
pub struct SampleRate {
    pub rate: u32,
}

#[derive(Debug, Clone)]
pub struct Drivers;

#[derive(Debug)]
struct DriverWrapper {
    drivers: Option<ai::AsioDrivers>,
    state: AsioState,
}

#[derive(Debug)]
enum AsioState {
    Offline,
    Loaded,
    Initialized,
    Prepared,
    Running,
}

pub struct AsioStreams {
    pub input: Option<AsioStream>,
    pub output: Option<AsioStream>,
}

pub struct AsioStream {
    pub buffer_infos: Vec<AsioBufferInfo>,
    pub buffer_size: i32,
}

// This is a direct copy of the ASIOSampleType
// inside ASIO SDK.
#[derive(Debug, FromPrimitive)]
#[repr(C)]
pub enum AsioSampleType {
    ASIOSTInt16MSB = 0,
    ASIOSTInt24MSB = 1, // used for 20 bits as well
    ASIOSTInt32MSB = 2,
    ASIOSTFloat32MSB = 3, // IEEE 754 32 bit float
    ASIOSTFloat64MSB = 4, // IEEE 754 64 bit double float

    // these are used for 32 bit data buffer, with different alignment of the data inside
    // 32 bit PCI bus systems can be more easily used with these
    ASIOSTInt32MSB16 = 8,  // 32 bit data with 16 bit alignment
    ASIOSTInt32MSB18 = 9,  // 32 bit data with 18 bit alignment
    ASIOSTInt32MSB20 = 10, // 32 bit data with 20 bit alignment
    ASIOSTInt32MSB24 = 11, // 32 bit data with 24 bit alignment

    ASIOSTInt16LSB = 16,
    ASIOSTInt24LSB = 17, // used for 20 bits as well
    ASIOSTInt32LSB = 18,
    ASIOSTFloat32LSB = 19, // IEEE 754 32 bit float, as found on Intel x86 architecture
    ASIOSTFloat64LSB = 20, // IEEE 754 64 bit double float, as found on Intel x86 architecture

    // these are used for 32 bit data buffer, with different alignment of the data inside
    // 32 bit PCI bus systems can more easily used with these
    ASIOSTInt32LSB16 = 24, // 32 bit data with 18 bit alignment
    ASIOSTInt32LSB18 = 25, // 32 bit data with 18 bit alignment
    ASIOSTInt32LSB20 = 26, // 32 bit data with 20 bit alignment
    ASIOSTInt32LSB24 = 27, // 32 bit data with 24 bit alignment

    //	ASIO DSD format.
    ASIOSTDSDInt8LSB1 = 32, // DSD 1 bit data, 8 samples per byte. First sample in Least significant bit.
    ASIOSTDSDInt8MSB1 = 33, // DSD 1 bit data, 8 samples per byte. First sample in Most significant bit.
    ASIOSTDSDInt8NER8 = 40, // DSD 8 bit data, 1 sample per byte. No Endianness required.

    ASIOSTLastEntry,
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct AsioBufferInfo {
    pub is_input: c_long,
    pub channel_num: c_long,
    pub buffers: [*mut std::os::raw::c_void; 2],
}

#[repr(C)]
struct AsioCallbacks {
    buffer_switch: extern "C" fn(double_buffer_index: c_long, direct_process: c_long) -> (),
    sample_rate_did_change: extern "C" fn(s_rate: c_double) -> (),
    asio_message:
        extern "C" fn(selector: c_long, value: c_long, message: *mut (), opt: *mut c_double)
            -> c_long,
    buffer_switch_time_info: extern "C" fn(
        params: *mut ai::ASIOTime,
        double_buffer_index: c_long,
        direct_process: c_long,
    ) -> *mut ai::ASIOTime,
}
extern "C" fn buffer_switch(double_buffer_index: c_long, direct_process: c_long) -> () {
    let mut bcs = buffer_callback.lock().unwrap();

    for mut bc in bcs.iter_mut() {
        if let Some(ref mut bc) = bc {
            bc.run(double_buffer_index);
        }
    }
}

extern "C" fn sample_rate_did_change(s_rate: c_double) -> () {}

extern "C" fn asio_message(
    selector: c_long, value: c_long, message: *mut (), opt: *mut c_double,
) -> c_long {
    4 as c_long
}

extern "C" fn buffer_switch_time_info(
    params: *mut ai::ASIOTime, double_buffer_index: c_long, direct_process: c_long,
) -> *mut ai::ASIOTime {
    params
}

fn get_drivers() -> MutexGuard<'static, DriverWrapper> {
    ASIO_DRIVERS.lock().unwrap()
}

impl Drivers {
    pub fn load(driver_name: &str) -> Result<Self, AsioDriverError> {
        let mut drivers = get_drivers();
        match drivers.state {
            AsioState::Offline => {
                // Make owned CString to send to load driver
                let mut my_driver_name =
                    CString::new(driver_name).expect("Can't go from str to CString");
                let raw = my_driver_name.into_raw();
                let mut driver_info = ai::ASIODriverInfo {
                    _bindgen_opaque_blob: [0u32; 43],
                };
                unsafe {
                    let mut asio_drivers = ai::AsioDrivers::new();
                    let load_result = asio_drivers.loadDriver(raw);
                    if load_result { drivers.state = AsioState::Loaded; }
                    let init_result = drivers.asio_init(&mut driver_info);
                    // Take back ownership
                    my_driver_name = CString::from_raw(raw);
                    if load_result && init_result.is_ok() {
                        println!("Creating drivers");
                        drivers.drivers = Some(asio_drivers);
                        STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                        Ok(Drivers)
                    } else {
                        Err(AsioDriverError::DriverLoadError)
                    }
                }
            },
            _ => {
                STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(Drivers)
            },
        }
    }

    /// Returns the channels for the driver it's passed
    ///
    /// # Arguments
    /// * `driver name` - Name of the driver
    /// # Usage
    /// Use the get_driver_list() to get the list of names
    /// Then pass the one you want to get_channels
    pub fn get_channels(&self) -> Channel {
        let channel: Channel;

        // Initialize memory for calls
        let mut ins: c_long = 0;
        let mut outs: c_long = 0;
        unsafe {
            get_drivers().asio_get_channels(&mut ins, &mut outs).expect("failed to get channels");
            channel = Channel {
                ins: ins as i64,
                outs: outs as i64,
            };
        }

        channel
    }

    pub fn get_sample_rate(&self) -> SampleRate {
        let sample_rate: SampleRate;

        // Initialize memory for calls
        let mut rate: c_double = 0.0f64;

        unsafe {
            get_drivers().asio_get_sample_rate(&mut rate).expect("failed to get sample rate");
            sample_rate = SampleRate { rate: rate as u32 };
        }

        sample_rate
    }

    pub fn get_data_type(&self) -> Result<AsioSampleType, AsioDriverError> {
        // Initialize memory for calls
        let mut channel_info = ai::ASIOChannelInfo {
            channel: 0,
            isInput: 0,
            isActive: 0,
            channelGroup: 0,
            type_: 0,
            name: [0 as c_char; 32],
        };
        unsafe {
            match get_drivers().asio_get_channel_info(&mut channel_info) {
                Ok(_) => {
                    num::FromPrimitive::from_i32(channel_info.type_)
                        .map_or(Err(AsioDriverError::TypeError), |t| Ok(t))
                },
                Err(e) => {
                    println!("Error getting data type {}", e);
                    Err(AsioDriverError::DriverLoadError)
                },
            } 
        }
    }

    pub fn prepare_input_stream(&self, output: Option<AsioStream>, mut num_channels: usize) -> Result<AsioStreams, AsioDriverError> {
        let mut buffer_infos = vec![
            AsioBufferInfo {
                is_input: 1,
                channel_num: 0,
                buffers: [std::ptr::null_mut(); 2],
            };
            num_channels
        ];
        
        let streams = AsioStreams{input: Some(AsioStream{buffer_infos, buffer_size: 0}), output};
        self.create_streams(streams)
    }

    /// Creates the output stream
    pub fn prepare_output_stream(&self, input: Option<AsioStream>, num_channels: usize) -> Result<AsioStreams, AsioDriverError> {
        // Initialize data for FFI
        let mut buffer_infos = vec![
            AsioBufferInfo {
                is_input: 0,
                channel_num: 0,
                buffers: [std::ptr::null_mut(); 2],
            };
            num_channels
        ];
        let streams = AsioStreams{output: Some(AsioStream{buffer_infos, buffer_size: 0}), input};
        self.create_streams(streams)
    }

    /// Creates the output stream
    fn create_streams(&self, streams: AsioStreams) -> Result<AsioStreams, AsioDriverError> {
        let AsioStreams {
            input,
            output,
        } = streams;
        match (input, output) {
            (Some(input), Some(mut output)) => {
                let split_point = input.buffer_infos.len();
                let mut bi = input.buffer_infos;
                bi.append(&mut output.buffer_infos);
                self.create_buffers(bi)
                    .map(|(mut bi, buffer_size)|{
                        let out_bi = bi.split_off(split_point);
                        let in_bi = bi;
                        let output = Some(AsioStream{
                            buffer_infos: out_bi,
                            buffer_size,
                        });
                        let input = Some(AsioStream{
                            buffer_infos: in_bi,
                            buffer_size,
                        });
                        AsioStreams{output, input}
                    })
            },
            (Some(input), None) => {
                self.create_buffers(input.buffer_infos)
                    .map(|(buffer_infos, buffer_size)| {
                        STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                        AsioStreams{
                            input: Some(AsioStream{
                                buffer_infos,
                                buffer_size,
                            }),
                            output: None,
                        }
                    })
            },
            (None, Some(output)) => {
                self.create_buffers(output.buffer_infos)
                    .map(|(buffer_infos, buffer_size)| {
                        STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                        AsioStreams{
                            output: Some(AsioStream{
                                buffer_infos,
                                buffer_size,
                            }),
                            input: None,
                        }
                    })
            },
            (None, None) => panic!("Trying to create streams without preparing"),
        }

    }

    fn create_buffers(&self, buffer_infos: Vec<AsioBufferInfo>) 
    -> Result<(Vec<AsioBufferInfo>, c_long), AsioDriverError>{
        let num_channels = buffer_infos.len();
        let mut callbacks = AsioCallbacks {
            buffer_switch: buffer_switch,
            sample_rate_did_change: sample_rate_did_change,
            asio_message: asio_message,
            buffer_switch_time_info: buffer_switch_time_info,
        };

        let mut min_b_size: c_long = 0;
        let mut max_b_size: c_long = 0;
        let mut pref_b_size: c_long = 0;
        let mut grans: c_long = 0;

        let mut drivers = get_drivers();

        unsafe {
            // Get the buffer sizes
            // min possilbe size
            // max possible size
            // preferred size
            // granularity
            drivers.asio_get_buffer_size(
                &mut min_b_size,
                &mut max_b_size,
                &mut pref_b_size,
                &mut grans,
            ).expect("Failed getting buffers");
            if pref_b_size > 0 {
                let mut buffer_info_convert: Vec<ai::ASIOBufferInfo> = buffer_infos
                    .into_iter()
                    .map(|bi| mem::transmute::<AsioBufferInfo, ai::ASIOBufferInfo>(bi))
                    .collect();
                let mut callbacks_convert =
                    mem::transmute::<AsioCallbacks, ai::ASIOCallbacks>(callbacks);
                drivers.asio_create_buffers(
                    buffer_info_convert.as_mut_ptr(),
                    num_channels as i32,
                    pref_b_size,
                    &mut callbacks_convert,
                ).map(|_|{
                    let mut buffer_infos: Vec<AsioBufferInfo> = buffer_info_convert
                        .into_iter()
                        .map(|bi| mem::transmute::<ai::ASIOBufferInfo, AsioBufferInfo>(bi))
                        .collect();
                    for d in &buffer_infos {
                        println!("after {:?}", d);
                    }
                    println!("channels: {:?}", num_channels);

                    (buffer_infos, pref_b_size)
                }).map_err(|e|{
                AsioDriverError::BufferError(format!(
                    "failed to create buffers, error code: {:?}", e))
                })
            } else {
                Err(AsioDriverError::BufferError(
                    "bad buffer size".to_owned(),
                ))
            }
        }
    }
}

impl Drop for Drivers {
    fn drop(&mut self) {
        println!("dropping drivers");
        let count = STREAM_DRIVER_COUNT.fetch_sub(1, Ordering::SeqCst);
        if count == 1 {
            println!("Destroying driver");
            clean_up();
        }
    }
}

unsafe impl Send for DriverWrapper {}

impl BufferCallback {
    fn run(&mut self, index: i32) {
        let mut cb = &mut self.0;
        cb(index);
    }
}

unsafe impl Send for AsioStream {}

pub fn set_callback<F: 'static>(mut callback: F) -> ()
where
    F: FnMut(i32) + Send,
{
    let mut bc = buffer_callback.lock().unwrap();
    bc.push(Some(BufferCallback(Box::new(callback))));
}

/// Returns a list of all the ASIO drivers
//TODO this needs to not create and remove drivers
pub fn get_driver_list() -> Vec<String> {
    let mut driver_list: Vec<String> = Vec::new();

    let mut driver_names: [[c_char; MAX_DRIVER]; MAX_DRIVER] = [[0; MAX_DRIVER]; MAX_DRIVER];
    let mut p_driver_name: [*mut i8; MAX_DRIVER] = [0 as *mut i8; MAX_DRIVER];

    for i in 0 .. MAX_DRIVER {
        p_driver_name[i] = driver_names[i].as_mut_ptr();
    }

    unsafe {
        let mut asio_drivers = ai::AsioDrivers::new();

        let num_drivers =
            asio_drivers.getDriverNames(p_driver_name.as_mut_ptr(), MAX_DRIVER as i32);

        if num_drivers > 0 {
            for i in 0 .. num_drivers {
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

pub fn clean_up() {
    let mut drivers = get_drivers();
    match drivers.state {
        AsioState::Offline => (),
        AsioState::Loaded => {
            unsafe {
                let mut old_drivers = drivers.drivers.take().unwrap();
                ai::destruct_AsioDrivers(&mut old_drivers);
            }
            drivers.state = AsioState::Offline;
        },
        AsioState::Initialized => {
            unsafe {
                drivers.asio_exit().expect("Failed to exit asio");
                let mut old_drivers = drivers.drivers.take().unwrap();
                ai::destruct_AsioDrivers(&mut old_drivers);
            }
            drivers.state = AsioState::Offline;
        },
        AsioState::Prepared => {
            unsafe {
                drivers.asio_dispose_buffers().expect("Failed to dispose buffers");
                drivers.asio_exit().expect("Failed to exit asio");
                let mut old_drivers = drivers.drivers.take().unwrap();
                ai::destruct_AsioDrivers(&mut old_drivers);
            }
            drivers.state = AsioState::Offline;
        },
        AsioState::Running => {
            unsafe {
                drivers.asio_stop();
                drivers.asio_dispose_buffers().expect("Failed to dispose buffers");
                drivers.asio_exit().expect("Failed to exit asio");
                let mut old_drivers = drivers.drivers.take().unwrap();
                ai::destruct_AsioDrivers(&mut old_drivers);
            }
            drivers.state = AsioState::Offline;
        },
    }
}

pub fn play() {
    unsafe {
        let result = get_drivers().asio_start();
        println!("start result: {:?}", result);
    }
}

pub fn stop() {
    unsafe {
        let result = get_drivers().asio_stop();
        println!("stop result: {:?}", result);
    }
}

impl DriverWrapper {
unsafe fn asio_init(&mut self, di: &mut ai::ASIODriverInfo) -> Result<(), AsioError> {
    if let AsioState::Loaded = self.state {
        let result = ai::ASIOInit(di);
        asio_result!(result)
            .map(|_| self.state = AsioState::Initialized)
    }else{
        Ok(())
    }
}

unsafe fn asio_get_channels(&mut self, ins: &mut c_long, outs: &mut c_long) -> Result<(), AsioError> {
    if let AsioState::Offline = self.state {
        Err(AsioError::NoDrivers)
    } else {
        let result = ai::ASIOGetChannels(ins, outs);
        asio_result!(result)
    }
}

unsafe fn asio_get_sample_rate(&mut self, rate: &mut c_double) -> Result<(), AsioError> {
    if let AsioState::Offline = self.state {
        Err(AsioError::NoDrivers)
    } else {
        let result = ai::get_sample_rate(rate);
        asio_result!(result)
    }
}

unsafe fn asio_get_channel_info(&mut self, ci: &mut ai::ASIOChannelInfo) -> Result<(), AsioError> {
    if let AsioState::Offline = self.state {
        Err(AsioError::NoDrivers)
    } else {
        let result = ai::ASIOGetChannelInfo(ci);
        asio_result!(result)
    }
}

unsafe fn asio_get_buffer_size(
    &mut self, 
    min_b_size: &mut c_long, max_b_size: &mut c_long, pref_b_size: &mut c_long, grans: &mut c_long,
) -> Result<(), AsioError> {
    if let AsioState::Offline = self.state {
        Err(AsioError::NoDrivers)
    } else {
        let result = ai::ASIOGetBufferSize(
            min_b_size,
            max_b_size,
            pref_b_size,
            grans,
        );
        asio_result!(result)
    }
}

unsafe fn asio_create_buffers(
    &mut self, 
    buffer_info_convert: *mut ai::ASIOBufferInfo, num_channels: i32, pref_b_size: c_long,
    callbacks_convert: &mut ai::ASIOCallbacks,
) -> Result<(), AsioError> {
    use AsioState::*;
    match self.state {
        Offline | Loaded => return Err(AsioError::NoDrivers),
        Running => {
            self.asio_stop();
            self.asio_dispose_buffers().expect("Failed to dispose buffers");
            self.state = Initialized;
        },
        Prepared => {
            self.asio_dispose_buffers().expect("Failed to dispose buffers");
            self.state = Initialized;
        },
        _ => (),
    }
    let result = ai::ASIOCreateBuffers(
        buffer_info_convert,
        num_channels,
        pref_b_size,
        callbacks_convert,
    );
    asio_result!(result)
        .map(|_| self.state = AsioState::Prepared)
}

unsafe fn asio_dispose_buffers(&mut self) -> Result<(), AsioError> {
    use AsioState::*;
    match self.state {
        Offline | Loaded => return Err(AsioError::NoDrivers),
        Running | Prepared => (),
        Initialized => return Ok(()),
    }
    let result = ai::ASIODisposeBuffers();
    asio_result!(result)
        .map(|_| self.state = AsioState::Initialized)
}

unsafe fn asio_exit(&mut self) -> Result<(), AsioError> {
    use AsioState::*;
    match self.state {
        Offline | Loaded => return Err(AsioError::NoDrivers),
        _ => (),
    }
    let result = ai::ASIOExit();
    asio_result!(result)
        .map(|_| self.state = AsioState::Loaded)
}

unsafe fn asio_start(&mut self) -> Result<(), AsioError> {
    use AsioState::*;
    match self.state {
        Offline | Loaded | Initialized => return Err(AsioError::NoDrivers),
        Running => return Ok(()),
        Prepared => (),
    }
    let result = ai::ASIOStart();
    asio_result!(result)
        .map(|_| self.state = AsioState::Running)
}

unsafe fn asio_stop(&mut self) -> Result<(), AsioError> {
    use AsioState::*;
    match self.state {
        Offline | Loaded => return Err(AsioError::NoDrivers),
        Running => (),
        Initialized | Prepared => return Ok(()),
    }
    let result = ai::ASIOStop();
    asio_result!(result)
        .map(|_| self.state = AsioState::Prepared)
}
}
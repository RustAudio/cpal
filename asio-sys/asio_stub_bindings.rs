// Stub bindings for docs.rs
// These are minimal type and function definitions to allow documentation generation
// without requiring the actual ASIO SDK.

use std::os::raw::{c_char, c_double, c_void};

// On Windows (the only platform where ASIO actually runs), c_long is i32.
// On non-Windows platforms (for docs.rs and local testing), redefine c_long as i32 to match.
#[cfg(target_os = "windows")]
use std::os::raw::c_long;
#[cfg(not(target_os = "windows"))]
type c_long = i32;

pub type ASIOBool = c_long;
pub type ASIOError = c_long;
pub type ASIOSampleRate = c_double;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ASIOSamples {
    pub hi: u32,
    pub lo: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ASIOTimeStamp {
    pub hi: u32,
    pub lo: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ASIODriverInfo {
    pub asioVersion: c_long,
    pub driverVersion: c_long,
    pub name: [c_char; 32],
    pub errorMessage: [c_char; 124],
    pub sysRef: *mut c_void,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ASIOChannelInfo {
    pub channel: c_long,
    pub isInput: ASIOBool,
    pub isActive: ASIOBool,
    pub channelGroup: c_long,
    pub type_: c_long,
    pub name: [c_char; 32],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ASIOBufferInfo {
    pub isInput: ASIOBool,
    pub channelNum: c_long,
    pub buffers: [*mut c_void; 2],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ASIOCallbacks {
    pub bufferSwitch: *const c_void,
    pub sampleRateDidChange: *const c_void,
    pub asioMessage: *const c_void,
    pub bufferSwitchTimeInfo: *const c_void,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AsioTimeInfo {
    pub speed: c_double,
    pub systemTime: ASIOTimeStamp,
    pub samplePosition: ASIOSamples,
    pub sampleRate: ASIOSampleRate,
    pub flags: c_long,
    pub reserved: [c_char; 12],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ASIOTimeCode {
    pub speed: c_double,
    pub timeCodeSamples: ASIOSamples,
    pub flags: c_long,
    pub future: [c_char; 64],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ASIOTime {
    pub reserved: [c_long; 4],
    pub timeInfo: AsioTimeInfo,
    pub timeCode: ASIOTimeCode,
}

#[repr(transparent)]
#[derive(Debug, Copy, Clone)]
pub struct AsioTimeInfoFlags(pub u32);

impl AsioTimeInfoFlags {
    pub const kSystemTimeValid: Self = Self(1);
    pub const kSamplePositionValid: Self = Self(1 << 1);
}

impl std::ops::BitOr for AsioTimeInfoFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

#[repr(transparent)]
#[derive(Debug, Copy, Clone)]
pub struct ASIOTimeCodeFlags(pub u32);

// Stub functions (will never be called on docs.rs)
#[no_mangle]
pub unsafe extern "C" fn ASIOInit(_info: *mut ASIODriverInfo) -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOExit() -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOStart() -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOStop() -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOGetChannels(_ins: *mut c_long, _outs: *mut c_long) -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOGetChannelInfo(_info: *mut ASIOChannelInfo) -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOCreateBuffers(
    _infos: *mut ASIOBufferInfo,
    _num: c_long,
    _size: c_long,
    _callbacks: *mut ASIOCallbacks,
) -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIODisposeBuffers() -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOGetBufferSize(
    _min: *mut c_long,
    _max: *mut c_long,
    _pref: *mut c_long,
    _gran: *mut c_long,
) -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOGetSamplePosition(
    _pos: *mut ASIOSamples,
    _stamp: *mut ASIOTimeStamp,
) -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn ASIOOutputReady() -> ASIOError {
    0
}

#[no_mangle]
pub unsafe extern "C" fn get_driver_names(_names: *mut *mut c_char, _max: c_long) -> c_long {
    0
}
#[no_mangle]
pub unsafe extern "C" fn load_asio_driver(_name: *mut c_char) -> bool {
    false
}
#[no_mangle]
pub unsafe extern "C" fn remove_current_driver() {}
#[no_mangle]
pub unsafe extern "C" fn get_sample_rate(_rate: *mut c_double) -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn set_sample_rate(_rate: c_double) -> ASIOError {
    0
}
#[no_mangle]
pub unsafe extern "C" fn can_sample_rate(_rate: c_double) -> ASIOError {
    0
}

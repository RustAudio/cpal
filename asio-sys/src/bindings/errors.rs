use std::error::Error;
use std::fmt;

/// Errors that might occur during `Asio::load_driver`.
#[derive(Debug)]
pub enum LoadDriverError {
    LoadDriverFailed,
    DriverAlreadyExists,
    InitializationFailed(AsioError),
}

/// General errors returned by ASIO.
#[derive(Debug)]
pub enum AsioError {
    NoDrivers,
    HardwareMalfunction,
    InvalidInput,
    BadMode,
    HardwareStuck,
    NoRate,
    ASE_NoMemory,
    InvalidBufferSize,
    UnknownError,
}

#[derive(Debug)]
pub enum AsioErrorWrapper {
    ASE_OK = 0,               // This value will be returned whenever the call succeeded
    ASE_SUCCESS = 0x3f4847a0, // unique success return value for ASIOFuture calls
    ASE_NotPresent = -1000,   // hardware input or output is not present or available
    ASE_HWMalfunction,        // hardware is malfunctioning (can be returned by any ASIO function)
    ASE_InvalidParameter,     // input parameter invalid
    ASE_InvalidMode,          // hardware is in a bad mode or used in a bad mode
    ASE_SPNotAdvancing,       // hardware is not running when sample position is inquired
    ASE_NoClock,              // sample clock or rate cannot be determined or is not present
    ASE_NoMemory,             // not enough memory for completing the request
    Invalid,
}

impl fmt::Display for LoadDriverError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl fmt::Display for AsioError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            AsioError::NoDrivers => {
                write!(f, "hardware input or output is not present or available")
            }
            AsioError::HardwareMalfunction => write!(
                f,
                "hardware is malfunctioning (can be returned by any ASIO function)"
            ),
            AsioError::InvalidInput => write!(f, "input parameter invalid"),
            AsioError::BadMode => write!(f, "hardware is in a bad mode or used in a bad mode"),
            AsioError::HardwareStuck => write!(
                f,
                "hardware is not running when sample position is inquired"
            ),
            AsioError::NoRate => write!(
                f,
                "sample clock or rate cannot be determined or is not present"
            ),
            AsioError::ASE_NoMemory => write!(f, "not enough memory for completing the request"),
            AsioError::InvalidBufferSize => write!(f, "buffersize out of range for device"),
            AsioError::UnknownError => write!(f, "Error not in SDK"),
        }
    }
}

impl Error for LoadDriverError {
    fn description(&self) -> &str {
        match *self {
            LoadDriverError::LoadDriverFailed => {
                "ASIO `loadDriver` function returned `false` indicating failure"
            }
            LoadDriverError::InitializationFailed(ref err) => err.description(),
            LoadDriverError::DriverAlreadyExists => {
                "ASIO only supports loading one driver at a time"
            }
        }
    }
}

impl Error for AsioError {
    fn description(&self) -> &str {
        match *self {
            AsioError::NoDrivers => "hardware input or output is not present or available",
            AsioError::HardwareMalfunction => {
                "hardware is malfunctioning (can be returned by any ASIO function)"
            }
            AsioError::InvalidInput => "input parameter invalid",
            AsioError::BadMode => "hardware is in a bad mode or used in a bad mode",
            AsioError::HardwareStuck => "hardware is not running when sample position is inquired",
            AsioError::NoRate => "sample clock or rate cannot be determined or is not present",
            AsioError::ASE_NoMemory => "not enough memory for completing the request",
            AsioError::InvalidBufferSize => "buffersize out of range for device",
            AsioError::UnknownError => "Error not in SDK",
        }
    }
}

impl From<AsioError> for LoadDriverError {
    fn from(err: AsioError) -> Self {
        LoadDriverError::InitializationFailed(err)
    }
}

macro_rules! asio_result {
    ($e:expr) => {{
        let res = { $e };
        match res {
            r if r == AsioErrorWrapper::ASE_OK as i32 => Ok(()),
            r if r == AsioErrorWrapper::ASE_SUCCESS as i32 => Ok(()),
            r if r == AsioErrorWrapper::ASE_NotPresent as i32 => Err(AsioError::NoDrivers),
            r if r == AsioErrorWrapper::ASE_HWMalfunction as i32 => {
                Err(AsioError::HardwareMalfunction)
            }
            r if r == AsioErrorWrapper::ASE_InvalidParameter as i32 => Err(AsioError::InvalidInput),
            r if r == AsioErrorWrapper::ASE_InvalidMode as i32 => Err(AsioError::BadMode),
            r if r == AsioErrorWrapper::ASE_SPNotAdvancing as i32 => Err(AsioError::HardwareStuck),
            r if r == AsioErrorWrapper::ASE_NoClock as i32 => Err(AsioError::NoRate),
            r if r == AsioErrorWrapper::ASE_NoMemory as i32 => Err(AsioError::ASE_NoMemory),
            _ => Err(AsioError::UnknownError),
        }
    }};
}

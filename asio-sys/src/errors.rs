use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum AsioDriverError {
    NoResult(String),
    BufferError(String),
    DriverLoadError,
    TypeError,
}

#[derive(Debug)]
pub enum AsioError {
            NoDrivers, 
            HardwareMalfunction,
            InvalidInput, 
            BadMode, 
            HardwareStuck, 
            NoRate, 
            ASE_NoMemory, 
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

impl fmt::Display for AsioDriverError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            AsioDriverError::NoResult(ref e) => write!(f, "Driver {} not found", e),
            AsioDriverError::BufferError(ref e) => write!(f, "Buffer Error: {}", e),
            AsioDriverError::DriverLoadError => write!(f, "Couldn't load the driver"),
            AsioDriverError::TypeError => write!(f, "Couldn't convert sample type"),
        }
    }
}

impl Error for AsioDriverError {
    fn description(&self) -> &str {
        match *self {
            AsioDriverError::NoResult(_) => "Couln't find driver",
            AsioDriverError::BufferError(_) => "Error creating the buffer",
            AsioDriverError::DriverLoadError => "Error loading the driver",
            AsioDriverError::TypeError => "Error getting sample type",
        }
    }
}

impl fmt::Display for AsioError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            AsioError::NoDrivers => {
                write!(f, "hardware input or output is not present or available")
            },
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
            AsioError::UnknownError => write!(f, "Error not in SDK"),
        }
    }
}

impl Error for AsioError {
    fn description(&self) -> &str {
        match *self {
            AsioError::NoDrivers => "hardware input or output is not present or available",
            AsioError::HardwareMalfunction => {
                "hardware is malfunctioning (can be returned by any ASIO function)"
            },
            AsioError::InvalidInput => "input parameter invalid",
            AsioError::BadMode => "hardware is in a bad mode or used in a bad mode",
            AsioError::HardwareStuck => "hardware is not running when sample position is inquired",
            AsioError::NoRate => "sample clock or rate cannot be determined or is not present",
            AsioError::ASE_NoMemory => "not enough memory for completing the request",
            AsioError::UnknownError => "Error not in SDK",
        }
    }
}
macro_rules! asio_error_helper {
    ($x:expr, $ae:ident{ $($v:ident),+ }, $inval:ident) => {
        match $x {
            $(_ if $x == $ae::$v as i32 => $ae::$v,)+
            _ => $ae::$inval,
        }
    };
}

macro_rules! asio_result {
    ($result:expr) => {
        match asio_error_helper!(
            $result,
            AsioErrorWrapper {
                ASE_OK,
                ASE_SUCCESS,
                ASE_NotPresent,
                ASE_HWMalfunction,
                ASE_InvalidParameter,
                ASE_InvalidMode,
                ASE_SPNotAdvancing,
                ASE_NoClock,
                ASE_NoMemory
            },
            Invalid
        ) {
            AsioErrorWrapper::ASE_OK => Ok(()),
            AsioErrorWrapper::ASE_SUCCESS => Ok(()),
            AsioErrorWrapper::ASE_NotPresent => Err(AsioError::NoDrivers),
            AsioErrorWrapper::ASE_HWMalfunction => Err(AsioError::HardwareMalfunction),
            AsioErrorWrapper::ASE_InvalidParameter => Err(AsioError::InvalidInput),
            AsioErrorWrapper::ASE_InvalidMode => Err(AsioError::BadMode),
            AsioErrorWrapper::ASE_SPNotAdvancing => Err(AsioError::HardwareStuck),
            AsioErrorWrapper::ASE_NoClock => Err(AsioError::NoRate),
            AsioErrorWrapper::ASE_NoMemory => Err(AsioError::ASE_NoMemory),
            AsioErrorWrapper::Invalid => Err(AsioError::UnknownError),
        }
    };
}

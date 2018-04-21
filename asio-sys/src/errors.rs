use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum ASIOError {
    NoResult(String),
    BufferError(String),
    DriverLoadError,
    TypeError,
}

impl fmt::Display for ASIOError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ASIOError::NoResult(ref e) => write!(f, "Driver {} not found", e),
            ASIOError::BufferError(ref e) => write!(f, "Buffer Error: {}", e),
            ASIOError::DriverLoadError => write!(f, "Couldn't load the driver"),
            ASIOError::TypeError => write!(f, "Couldn't convert sample type"),
        }
    }
}

impl Error for ASIOError {
    fn description(&self) -> &str {
        match *self {
            ASIOError::NoResult(_) => "Couln't find driver",
            ASIOError::BufferError(_) => "Error creating the buffer",
            ASIOError::DriverLoadError => "Error loading the driver",
            ASIOError::TypeError => "Error getting sample type",
        }
    }
}

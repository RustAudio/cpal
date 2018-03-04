use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum ASIOError{
    NoResult(String),
}

impl fmt::Display for ASIOError{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result{
        match *self{
            ASIOError::NoResult(ref e) => write!(f, "Driver {} not found", e),
        }
    }

}

impl Error for ASIOError{
    fn description(&self) -> &str {
        match *self{
            ASIOError::NoResult(_) => "Couln't find driver",
        }
    }
}

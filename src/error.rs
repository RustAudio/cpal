use failure::{Context, Fail, Backtrace};
use std::fmt::{self, Display};
use std::result::Result as StdResult;

/// The type of all errors in this library.
#[derive(Debug)]
pub struct Error {
    inner: Context<ErrorKind>
}

/// The possible kinds of errors that functions in this library can return
#[derive(Copy, Clone, Eq, PartialEq, Debug, Fail)]
pub enum ErrorKind {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(display="device not available")]
    DeviceNotAvailable,
    /// Returned if e.g. the default input format was requested on an output-only audio device.
    #[fail(display="stream type not supported")]
    StreamTypeNotSupported,
    /// The required format is not supported.
    #[fail(display="format not supported")]
    FormatNotSupported,
    /// There was an error getting the minimum supported sample rate.
    #[fail(display="error getting minimum supported rate")]
    CannotGetMinimumSupportedRate,
    /// There was an error getting the maximum supported sample rate.
    #[fail(display="error getting maximum supported rate")]
    CannotGetMaximumSupportedRate,
    /// Tried to create a C string from a rust String with a null byte (`\0`).
    #[fail(display="tried to create a C style string from a string with a null byte")]
    NullInString,
}

impl Error {
    /// Get the kind of this error
    pub fn kind(&self) -> &ErrorKind {
        self.inner.get_context()
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Error { inner: Context::new(kind) }
    }
}

impl From<Context<ErrorKind>> for Error {
    fn from(inner: Context<ErrorKind>) -> Self {
        Error { inner }
    }
}

impl Fail for Error {
    fn cause(&self) -> Option<&Fail> {
        self.inner.cause()
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        self.inner.backtrace()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

pub type Result<T> = StdResult<T, Error>;

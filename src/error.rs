use failure::Fail;

/// The requested host, although supported on this platform, is unavailable.
#[derive(Clone, Debug, Fail)]
#[fail(display = "the requested host is unavailable")]
pub struct HostUnavailable;

/// Some error has occurred that is specific to the backend from which it was produced.
///
/// This error is often used as a catch-all in cases where:
///
/// - It is unclear exactly what error might be produced by the backend API.
/// - It does not make sense to add a variant to the enclosing error type.
/// - No error was expected to occur at all, but we return an error to avoid the possibility of a
///   `panic!` caused by some unforseen or unknown reason.
///
/// **Note:** If you notice a `BackendSpecificError` that you believe could be better handled in a
/// cross-platform manner, please create an issue or submit a pull request with a patch that adds
/// the necessary error variant to the appropriate error enum.
#[derive(Clone, Debug, Fail)]
#[fail(display = "A backend-specific error has occurred: {}", description)]
pub struct BackendSpecificError {
    pub description: String,
}

/// An error that might occur while attempting to enumerate the available devices on a system.
#[derive(Debug, Fail)]
pub enum DevicesError {
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    },
}

/// An error that may occur while attempting to retrieve a device name.
#[derive(Debug, Fail)]
pub enum DeviceNameError {
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    },
}

/// Error that can happen when enumerating the list of supported formats.
#[derive(Debug, Fail)]
pub enum SupportedFormatsError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(
        display = "The requested device is no longer available. For example, it has been unplugged."
    )]
    DeviceNotAvailable,
    /// We called something the C-Layer did not understand
    #[fail(
        display = "Invalid argument passed to the backend. For example, this happens when trying to read capture capabilities when the device does not support it."
    )]
    InvalidArgument,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    },
}

/// May occur when attempting to request the default input or output stream format from a `Device`.
#[derive(Debug, Fail)]
pub enum DefaultFormatError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(
        display = "The requested device is no longer available. For example, it has been unplugged."
    )]
    DeviceNotAvailable,
    /// Returned if e.g. the default input format was requested on an output-only audio device.
    #[fail(display = "The requested stream type is not supported by the device.")]
    StreamTypeNotSupported,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    },
}

/// Error that can happen when creating a `Stream`.
#[derive(Debug, Fail)]
pub enum BuildStreamError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(
        display = "The requested device is no longer available. For example, it has been unplugged."
    )]
    DeviceNotAvailable,
    /// The required format is not supported.
    #[fail(display = "The requested stream format is not supported by the device.")]
    FormatNotSupported,
    /// We called something the C-Layer did not understand
    ///
    /// On ALSA device functions called with a feature they do not support will yield this. E.g.
    /// Trying to use capture capabilities on an output only format yields this.
    #[fail(display = "The requested device does not support this capability (invalid argument)")]
    InvalidArgument,
    /// Occurs if adding a new Stream ID would cause an integer overflow.
    #[fail(display = "Adding a new stream ID would cause an overflow")]
    StreamIdOverflow,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    },
}

/// Errors that might occur when calling `play_stream`.
///
/// As of writing this, only macOS may immediately return an error while calling this method. This
/// is because both the alsa and wasapi backends only enqueue these commands and do not process
/// them immediately.
#[derive(Debug, Fail)]
pub enum PlayStreamError {
    /// The device associated with the stream is no longer available.
    #[fail(display = "the device associated with the stream is no longer available")]
    DeviceNotAvailable,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    },
}

/// Errors that might occur when calling `pause_stream`.
///
/// As of writing this, only macOS may immediately return an error while calling this method. This
/// is because both the alsa and wasapi backends only enqueue these commands and do not process
/// them immediately.
#[derive(Debug, Fail)]
pub enum PauseStreamError {
    /// The device associated with the stream is no longer available.
    #[fail(display = "the device associated with the stream is no longer available")]
    DeviceNotAvailable,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    },
}

/// Errors that might occur while a stream is running.
#[derive(Debug, Fail)]
pub enum StreamError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[fail(
        display = "The requested device is no longer available. For example, it has been unplugged."
    )]
    DeviceNotAvailable,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[fail(display = "{}", err)]
    BackendSpecific {
        #[fail(cause)]
        err: BackendSpecificError,
    },
}

impl From<BackendSpecificError> for DevicesError {
    fn from(err: BackendSpecificError) -> Self {
        DevicesError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for DeviceNameError {
    fn from(err: BackendSpecificError) -> Self {
        DeviceNameError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for SupportedFormatsError {
    fn from(err: BackendSpecificError) -> Self {
        SupportedFormatsError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for DefaultFormatError {
    fn from(err: BackendSpecificError) -> Self {
        DefaultFormatError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for BuildStreamError {
    fn from(err: BackendSpecificError) -> Self {
        BuildStreamError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for PlayStreamError {
    fn from(err: BackendSpecificError) -> Self {
        PlayStreamError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for PauseStreamError {
    fn from(err: BackendSpecificError) -> Self {
        PauseStreamError::BackendSpecific { err }
    }
}

impl From<BackendSpecificError> for StreamError {
    fn from(err: BackendSpecificError) -> Self {
        StreamError::BackendSpecific { err }
    }
}

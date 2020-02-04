use thiserror::Error;

/// The requested host, although supported on this platform, is unavailable.
#[derive(Clone, Debug, Error)]
#[error("the requested host is unavailable")]
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
#[derive(Clone, Debug, Error)]
#[error("A backend-specific error has occurred: {description}")]
pub struct BackendSpecificError {
    pub description: String,
}

/// An error that might occur while attempting to enumerate the available devices on a system.
#[derive(Debug, Error)]
pub enum DevicesError {
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[error("{err}")]
    BackendSpecific {
        #[from]
        err: BackendSpecificError,
    },
}

/// An error that may occur while attempting to retrieve a device name.
#[derive(Debug, Error)]
pub enum DeviceNameError {
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[error("{err}")]
    BackendSpecific {
        #[from]
        err: BackendSpecificError,
    },
}

/// Error that can happen when enumerating the list of supported formats.
#[derive(Debug, Error)]
pub enum SupportedStreamConfigsError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[error("The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,
    /// We called something the C-Layer did not understand
    #[error(
        "Invalid argument passed to the backend. For example, this happens when trying to read capture capabilities when the device does not support it."
    )]
    InvalidArgument,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[error("{err}")]
    BackendSpecific {
        #[from]
        err: BackendSpecificError,
    },
}

/// May occur when attempting to request the default input or output stream format from a `Device`.
#[derive(Debug, Error)]
pub enum DefaultStreamConfigError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[error("The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,
    /// Returned if e.g. the default input format was requested on an output-only audio device.
    #[error("The requested stream type is not supported by the device.")]
    StreamTypeNotSupported,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[error("{err}")]
    BackendSpecific {
        #[from]
        err: BackendSpecificError,
    },
}

/// Error that can happen when creating a `Stream`.
#[derive(Debug, Error)]
pub enum BuildStreamError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[error("The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,
    /// The specified stream configuration is not supported.
    #[error("The requested stream configuration is not supported by the device.")]
    StreamConfigNotSupported,
    /// We called something the C-Layer did not understand
    ///
    /// On ALSA device functions called with a feature they do not support will yield this. E.g.
    /// Trying to use capture capabilities on an output only format yields this.
    #[error("The requested device does not support this capability (invalid argument)")]
    InvalidArgument,
    /// Occurs if adding a new Stream ID would cause an integer overflow.
    #[error("Adding a new stream ID would cause an overflow")]
    StreamIdOverflow,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[error("{err}")]
    BackendSpecific {
        #[from]
        err: BackendSpecificError,
    },
}

/// Errors that might occur when calling `play_stream`.
///
/// As of writing this, only macOS may immediately return an error while calling this method. This
/// is because both the alsa and wasapi backends only enqueue these commands and do not process
/// them immediately.
#[derive(Debug, Error)]
pub enum PlayStreamError {
    /// The device associated with the stream is no longer available.
    #[error("the device associated with the stream is no longer available")]
    DeviceNotAvailable,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[error("{err}")]
    BackendSpecific {
        #[from]
        err: BackendSpecificError,
    },
}

/// Errors that might occur when calling `pause_stream`.
///
/// As of writing this, only macOS may immediately return an error while calling this method. This
/// is because both the alsa and wasapi backends only enqueue these commands and do not process
/// them immediately.
#[derive(Debug, Error)]
pub enum PauseStreamError {
    /// The device associated with the stream is no longer available.
    #[error("the device associated with the stream is no longer available")]
    DeviceNotAvailable,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[error("{err}")]
    BackendSpecific {
        #[from]
        err: BackendSpecificError,
    },
}

/// Errors that might occur while a stream is running.
#[derive(Debug, Error)]
pub enum StreamError {
    /// The device no longer exists. This can happen if the device is disconnected while the
    /// program is running.
    #[error("The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,
    /// See the `BackendSpecificError` docs for more information about this error variant.
    #[error("{err}")]
    BackendSpecific {
        #[from]
        err: BackendSpecificError,
    },
}

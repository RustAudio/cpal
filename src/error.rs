use std::{
    borrow::Cow,
    error::Error as StdError,
    fmt::{Display, Formatter},
};

/// A list specifying general categories of CPAL error.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The device is temporarily busy. This can happen when another application or stream
    /// is using the device. Retrying after a short delay may succeed.
    DeviceBusy,

    /// The requested audio device is not available.
    ///
    /// This can happen if the device has been disconnected while the program is running, or if
    /// the device identifier refers to a device that does not exist on this system.
    DeviceNotAvailable,

    /// The audio host (server or subsystem) is not available on this system.
    ///
    /// This is distinct from [`DeviceNotAvailable`]: when a host (e.g. PulseAudio, PipeWire, JACK,
    /// or kernel subsystem) is absent or not running, no devices can be reached through it.
    ///
    /// [`DeviceNotAvailable`]: ErrorKind::DeviceNotAvailable
    HostUnavailable,

    /// Invalid input or argument.
    InvalidInput,

    /// Access to the device or resource was denied by the operating system or audio subsystem.
    ///
    /// The device exists and may be functional, but the current process or user does not have
    /// permission to use it. Common causes include microphone privacy settings (iOS, macOS),
    /// missing audio group membership (Linux), or file permission errors.
    ///
    /// Unlike [`DeviceNotAvailable`], which signals absence, this variant signals an
    /// authorization failure.
    ///
    /// [`DeviceNotAvailable`]: ErrorKind::DeviceNotAvailable
    PermissionDenied,

    /// The stream configuration is no longer valid and must be rebuilt.
    StreamInvalidated,

    /// The requested stream configuration is not supported. This includes unsupported sample
    /// rates, channel counts, or sample formats.
    UnsupportedConfig,

    /// The requested operation is not supported. This includes unsupported stream directions
    /// (e.g., requesting input on an output-only device), unavailable features, or operations
    /// not implemented by the backend.
    UnsupportedOperation,

    /// A buffer underrun or overrun occurred, causing a potential audio glitch.
    Xrun,

    /// The requested thread priority is unavailable for the audio callback thread.
    /// Audio will still play, but may be subject to increased latency or glitches under load.
    ThreadPriorityUnavailable,

    /// A catch-all for errors that do not fall under any other CPAL error kind.
    ///
    /// CPAL itself emits this variant only for genuinely unclassifiable conditions. Treat them as
    /// permanent: no retry strategy is possible without host-specific knowledge.
    ///
    /// New [`ErrorKind`] variants may be added in future releases to cover specific cases
    /// currently reported as `Other`.
    Other,
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HostUnavailable => f.write_str(
                "The requested audio host is not available. The subsystem or daemon may not be installed or running.",
            ),
            Self::DeviceNotAvailable => f.write_str(
                "The requested audio device is not available. It may have been disconnected.",
            ),
            Self::DeviceBusy => f.write_str(
                "The requested device is temporarily busy. Another application or stream may be using it.",
            ),
            Self::UnsupportedConfig => f.write_str(
                "The requested stream configuration is not supported by the device.",
            ),
            Self::UnsupportedOperation => f.write_str("The requested operation is not supported."),
            Self::InvalidInput => f.write_str("Invalid input or argument."),
            Self::StreamInvalidated => {
                f.write_str("The stream configuration is no longer valid and must be rebuilt.")
            }
            Self::Xrun => f.write_str("A buffer underrun or overrun occurred."),
            Self::PermissionDenied => f.write_str(
                "Permission denied. Grant the required access and retry.",
            ),
            Self::ThreadPriorityUnavailable => f.write_str(
                "Thread priority elevation is unavailable for the audio thread. \
                 Audio may be subject to increased latency or glitches under load.",
            ),
            Self::Other => f.write_str("An error occurred."),
        }
    }
}

/// Error type for all CPAL operations.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Error {
    kind: ErrorKind,
    message: Option<Cow<'static, str>>,
}

impl Error {
    /// Create a new error with the given kind and no message.
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            message: None,
        }
    }

    /// Create a new error with the given kind and a human-readable message.
    pub fn with_message(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            message: Some(message.into()),
        }
    }

    /// Returns the error kind.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns the human-readable message, if any.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.message {
            Some(msg) => f.write_str(msg),
            None => write!(f, "{}", self.kind),
        }
    }
}

impl StdError for Error {}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self::new(kind)
    }
}

#[cfg(all(
    feature = "audio_thread_priority",
    any(
        target_os = "windows",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    )
))]
impl From<audio_thread_priority::AudioThreadPriorityError> for Error {
    fn from(err: audio_thread_priority::AudioThreadPriorityError) -> Self {
        use std::error::Error as StdError;
        let msg = match err.source() {
            Some(inner) => {
                format!("Failed to promote audio thread to real-time priority: {err}: {inner}")
            }
            None => format!("Failed to promote audio thread to real-time priority: {err}"),
        };
        Error::with_message(ErrorKind::ThreadPriorityUnavailable, msg)
    }
}

/// Extension trait for attaching a context message to a [`Result`] whose error converts into
/// [`cpal::Error`].
#[allow(dead_code)]
pub(crate) trait ResultExt<T> {
    /// Converts the error via [`Into<cpal::Error>`] and prepends `msg`, yielding
    /// `"<msg>: <original error>"` as the message.
    fn context(self, msg: impl Display) -> Result<T, Error>;
}

impl<T, E: Into<Error>> ResultExt<T> for Result<T, E> {
    fn context(self, msg: impl Display) -> Result<T, Error> {
        self.map_err(|e| {
            let e = e.into();
            Error::with_message(e.kind(), format!("{msg}: {e}"))
        })
    }
}

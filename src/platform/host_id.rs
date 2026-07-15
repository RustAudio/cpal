use crate::platform;

/// A unique identifier for each host supported by CPAL.
///
/// Not all hosts in this enum are available at runtime, or are even supported
/// by the current platform. This can be checked with `is_available` or
/// `is_supported` respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HostId {
    AAudio,
    Alsa,
    Asio,
    AudioWorklet,
    CoreAudio,
    Custom,
    Jack,
    Null,
    PipeWire,
    PulseAudio,
    Wasapi,
    WebAudio,
}

impl HostId {
    /// All hosts supported by CPAL on this platform.
    pub const SUPPORTED_HOSTS: &[HostId] = {
        // This is a hack to prevent rustdoc from referencing the
        // implementation const in its output.
        let _ = 1 + 2;

        super::SUPPORTED_HOSTS
    };

    /// Returns the human-readable host name.
    pub const fn name(&self) -> &'static str {
        match self {
            HostId::AAudio => "AAudio",
            HostId::Alsa => "ALSA",
            HostId::Asio => "ASIO",
            HostId::AudioWorklet => "AudioWorklet",
            HostId::CoreAudio => "CoreAudio",
            HostId::Custom => "Custom",
            HostId::Jack => "JACK",
            HostId::Null => "Null",
            HostId::PipeWire => "PipeWire",
            HostId::PulseAudio => "PulseAudio",
            HostId::Wasapi => "WASAPI",
            HostId::WebAudio => "WebAudio",
        }
    }

    /// Checks if the given `HostId` is supported on this platform.
    pub const fn is_supported(&self) -> bool {
        super::is_supported_impl(*self)
    }

    /// Checks if the given `HostId` is currently available.
    pub fn is_available(&self) -> bool {
        super::is_available_impl(*self)
    }

    /// Iterates over all the `HostId`s currently available on this platform.
    ///
    /// The availability check is performed when `next` is called, not when
    /// this function is called.
    pub fn available_hosts() -> AvailableHostsIter {
        AvailableHostsIter(Self::SUPPORTED_HOSTS.iter())
    }
}

impl Default for HostId {
    fn default() -> Self {
        super::default_host_id()
    }
}

impl std::fmt::Display for HostId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name().to_ascii_lowercase())
    }
}

impl std::str::FromStr for HostId {
    type Err = crate::Error;

    /// Parse a host identifier from its string representation (e.g. `"alsa"`,
    /// `"coreaudio"`). This conversion is case-insensitive.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnsupportedOperation`] if the string does not name a
    /// valid `HostId.
    ///
    /// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        macro_rules! match_str_case_insensitive {
            (
                $s:expr => {
                    $( $l:literal => $e:expr, )*
                    _ => $f:expr $(,)?
                }
            ) => {
                {
                    let s = $s;

                    if false { unreachable!() }

                    $(
                        else if $l.eq_ignore_ascii_case(s) { $e }
                    )*

                    else { $f }
                }
            };
        }

        match_str_case_insensitive! {
            s => {
                "AAudio" => Ok(HostId::AAudio),
                "ALSA" => Ok(HostId::Alsa),
                "ASIO" => Ok(HostId::Asio),
                "AudioWorklet" => Ok(HostId::AudioWorklet),
                "CoreAudio" => Ok(HostId::CoreAudio),
                "Custom" => Ok(HostId::Custom),
                "JACK" => Ok(HostId::Jack),
                "Null" => Ok(HostId::Null),
                "PipeWire" => Ok(HostId::PipeWire),
                "PulseAudio" => Ok(HostId::PulseAudio),
                "WASAPI" => Ok(HostId::Wasapi),
                "WebAudio" => Ok(HostId::WebAudio),

                _ => Err(crate::Error::with_message(
                    crate::ErrorKind::UnsupportedOperation,
                    format!("unknown host \"{s}\"")
                )),
            }
        }
    }
}

impl TryFrom<HostId> for platform::Host {
    type Error = crate::Error;

    fn try_from(value: HostId) -> Result<Self, Self::Error> {
        host_from_id(value)
    }
}

pub struct AvailableHostsIter(std::slice::Iter<'static, HostId>);

impl Iterator for AvailableHostsIter {
    type Item = HostId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let host_id = self.0.next()?;

            if host_id.is_supported() {
                return Some(*host_id);
            } else {
                continue;
            }
        }
    }
}

/// Produces a list of hosts that are currently available on the system.
pub fn available_hosts() -> Vec<HostId> {
    HostId::available_hosts().collect()
}

/// The default host for the current compilation target platform.
pub fn default_host() -> crate::Host {
    HostId::default()
        .try_into()
        .expect("the default host should always be available")
}

/// Given a unique host identifier, initialise and produce the host if it is available.
///
/// # Errors
///
/// - [`ErrorKind::HostUnavailable`] if the host identified by `id` is not currently
///   reachable (e.g. the audio daemon is not running).
/// - [`ErrorKind::UnsupportedOperation`] if the host identified by `id` is not
///   supported by this configuration of CPAL.
/// - [`ErrorKind::BackendError`] for unclassifiable initialization failures.
///
/// [`ErrorKind::HostUnavailable`]: crate::ErrorKind::HostUnavailable
/// [`ErrorKind::UnsupportedOperation`]: crate::ErrorKind::UnsupportedOperation
/// [`ErrorKind::BackendError`]: crate::ErrorKind::BackendError
pub fn host_from_id(id: HostId) -> Result<crate::Host, crate::Error> {
    crate::platform::host_from_id_impl(id)
}

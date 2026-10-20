//! Names and roles that audio servers show for an application's streams.
//!
//! Volume mixers such as `pavucontrol` and session managers such as WirePlumber list each stream
//! with the application that owns it, an icon, the stream's own name, and a role that policies
//! use to route or duck it. These types carry that information to the hosts that have a place
//! for it. They are hints: a host without such a place ignores them.

/// Identifies the application that owns the streams.
///
/// The PipeWire and PulseAudio hosts take it when they are created, with their
/// `with_application` constructors, since PulseAudio names the application by its connection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ApplicationInfo {
    /// The application's name as users know it, such as `"Spotifast"`.
    pub name: String,
    /// A freedesktop icon name for the application, such as `"spotifast"`.
    pub icon_name: Option<String>,
}

/// Describes one stream.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct StreamMetadata {
    /// What the stream plays or records, such as a song title or `"Voice call"`. Without it,
    /// hosts keep their own default name.
    pub name: Option<String>,
    /// What kind of audio the stream carries.
    pub role: Option<StreamRole>,
}

/// What kind of audio a stream carries.
///
/// Session managers use it to pick a device or to lower other streams, for example while a
/// call is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StreamRole {
    /// Music playback.
    Music,
    /// The audio of a video or film.
    Movie,
    /// Game audio.
    Game,
    /// Calls and other two-way voice.
    Communication,
    /// Notifications, alerts and other short event sounds.
    Notification,
    /// Music production and other professional audio work.
    Production,
    /// Screen readers and other accessibility output.
    Accessibility,
}

impl ApplicationInfo {
    /// Returns an error when a value cannot be passed to the audio server.
    #[cfg_attr(
        not(any(
            test,
            all(
                any(
                    target_os = "linux",
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "netbsd"
                ),
                any(feature = "pipewire", feature = "pulseaudio")
            )
        )),
        expect(dead_code, reason = "only the PipeWire and PulseAudio hosts use it")
    )]
    pub(crate) fn validate(&self) -> Result<(), crate::Error> {
        validate_value(&self.name)?;
        self.icon_name.as_deref().map_or(Ok(()), validate_value)
    }
}

impl StreamMetadata {
    /// Returns an error when a value cannot be passed to the audio server.
    #[cfg_attr(
        not(any(
            test,
            all(
                any(
                    target_os = "linux",
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "netbsd"
                ),
                any(feature = "pipewire", feature = "pulseaudio")
            )
        )),
        expect(dead_code, reason = "only the PipeWire and PulseAudio hosts use it")
    )]
    pub(crate) fn validate(&self) -> Result<(), crate::Error> {
        self.name.as_deref().map_or(Ok(()), validate_value)
    }
}

fn validate_value(value: &str) -> Result<(), crate::Error> {
    if value.contains('\0') {
        return Err(crate::Error::with_message(
            crate::ErrorKind::InvalidInput,
            format!("{value:?} contains a NUL byte"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nul_bytes() {
        let app = ApplicationInfo {
            name: "Player".into(),
            icon_name: Some("pla\0yer".into()),
        };
        assert!(app.validate().is_err());
        let stream = StreamMetadata {
            name: Some("So\0ng".into()),
            role: None,
        };
        assert!(stream.validate().is_err());
        assert!(StreamMetadata::default().validate().is_ok());
    }
}

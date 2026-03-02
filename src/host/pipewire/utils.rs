pub const METADATA_NAME: &str = "metadata.name";
pub const PORT_GROUP: &str = "port.group";

// NOTE: the icon name contains bluetooth and etc, not icon-name, but icon_name
// I have tried to get the information, and get
// "device.icon-name": "audio-card-analog",
// "device.icon_name": "video-display",
// So seems the `icon_name` is usable
pub const DEVICE_ICON_NAME: &str = "device.icon_name";

pub mod clock {
    pub const RATE: &str = "clock.rate";
    pub const ALLOWED_RATES: &str = "clock.allowed-rates";
    pub const QUANTUM: &str = "clock.quantum";
    pub const MIN_QUANTUM: &str = "clock.min-quantum";
    pub const MAX_QUANTUM: &str = "clock.max-quantum";
}

pub mod audio {
    pub const SINK: &str = "Audio/Sink";
    pub const SOURCE: &str = "Audio/Source";
    pub const DUPLEX: &str = "Audio/Duplex";
    pub const STREAM_OUTPUT: &str = "Stream/Output/Audio";
    pub const STREAM_INPUT: &str = "Stream/Input/Audio";
}

pub mod group {
    pub const PLAY_BACK: &str = "playback";
    pub const CAPTURE: &str = "capture";
}

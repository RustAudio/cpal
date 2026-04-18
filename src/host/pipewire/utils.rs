use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const METADATA_NAME: &str = "metadata.name";

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

pub mod node {
    pub const RATE: &str = "node.rate";
    pub const LATENCY: &str = "node.latency";
}

pub mod audio {
    pub const SINK: &str = "Audio/Sink";
    pub const SOURCE: &str = "Audio/Source";
    pub const DUPLEX: &str = "Audio/Duplex";
    pub const STREAM_OUTPUT: &str = "Stream/Output/Audio";
    pub const STREAM_INPUT: &str = "Stream/Input/Audio";
}

/// Returns the path of the PipeWire socket, checking the standard locations
/// including the parent of `XDG_RUNTIME_DIR` for Snap sandboxes.
pub fn find_socket_path() -> Option<&'static PathBuf> {
    static SOCKET: OnceLock<Option<PathBuf>> = OnceLock::new();
    SOCKET
        .get_or_init(|| {
            fn socket_in(dir: &Path) -> Option<PathBuf> {
                let p = dir.join("pipewire-0");
                p.exists().then_some(p)
            }

            if let Ok(dir) = std::env::var("PIPEWIRE_RUNTIME_DIR") {
                if let Some(p) = socket_in(Path::new(&dir)) {
                    return Some(p);
                }
            }

            if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
                let path = Path::new(&xdg);
                if let Some(p) = socket_in(path) {
                    return Some(p);
                }
                // Snap sets XDG_RUNTIME_DIR to a snap-specific subdirectory but keeps
                // the PipeWire socket in the parent.
                if let Some(p) = path.parent().and_then(socket_in) {
                    return Some(p);
                }
            }

            None
        })
        .as_ref()
}

use std::path::PathBuf;
use crate::constants::*;

pub(super) struct PipewireClientConnectionString;

impl PipewireClientConnectionString {
    pub(super) fn from_env() -> String {
        let pipewire_runtime_dir = std::env::var(PIPEWIRE_RUNTIME_DIR_ENVIRONMENT_KEY);
        let xdg_runtime_dir = std::env::var(XDG_RUNTIME_DIR_ENVIRONMENT_KEY);

        let socket_directory = match (xdg_runtime_dir, pipewire_runtime_dir) {
            (Ok(value), Ok(_)) => value,
            (Ok(value), Err(_)) => value,
            (Err(_), Ok(value)) => value,
            (Err(_), Err(_)) => panic!(
                "${} or ${} should be set. See https://docs.pipewire.org/page_man_pipewire_1.html",
                PIPEWIRE_RUNTIME_DIR_ENVIRONMENT_KEY, XDG_RUNTIME_DIR_ENVIRONMENT_KEY
            ),
        };

        let pipewire_remote = match std::env::var(PIPEWIRE_REMOTE_ENVIRONMENT_KEY) {
            Ok(value) => value,
            Err(_) => panic!(
                "${PIPEWIRE_REMOTE_ENVIRONMENT_KEY} should be set. See https://docs.pipewire.org/page_man_pipewire_1.html",
            )
        };

        let socket_path = PathBuf::from(socket_directory).join(pipewire_remote);
        socket_path.to_str().unwrap().to_string()
    }
}

pub(super) struct PipewireClientInfo {
    pub name: String,
    pub connection_string: String,
}
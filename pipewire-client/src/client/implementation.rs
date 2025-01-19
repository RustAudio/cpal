extern crate pipewire;

use crate::client::api::{CoreApi, InternalApi, NodeApi, StreamApi};
use crate::client::connection_string::{PipewireClientInfo, PipewireClientSocketPath};
use crate::client::handlers::thread;
use crate::error::Error;
use crate::messages::{EventMessage, MessageRequest, MessageResponse};
use crate::states::GlobalObjectState;
use crate::utils::Backoff;
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::string::ToString;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

pub(super) static CLIENT_NAME_PREFIX: &str = "cpal-client";
pub(super) static CLIENT_INDEX: AtomicU32 = AtomicU32::new(0);

pub struct PipewireClient {
    pub(crate) name: String,
    socket_path: PathBuf,
    thread_handle: Option<JoinHandle<()>>,
    internal_api: Arc<InternalApi>,
    core_api: CoreApi,
    node_api: NodeApi,
    stream_api: StreamApi,
}

impl PipewireClient {
    pub fn new() -> Result<Self, Error> {
        let name = format!("{}-{}", CLIENT_NAME_PREFIX, CLIENT_INDEX.load(Ordering::SeqCst));
        CLIENT_INDEX.fetch_add(1, Ordering::SeqCst);

        let socket_path = PipewireClientSocketPath::from_env();

        let client_info = PipewireClientInfo {
            name: name.clone(),
            socket_location: socket_path.parent().unwrap().to_str().unwrap().to_string(),
            socket_name: socket_path.file_name().unwrap().to_str().unwrap().to_string(),
        };

        let (main_sender, main_receiver) = crossbeam_channel::unbounded();
        let (pw_sender, pw_receiver) = pipewire::channel::channel();
        let (event_sender, event_receiver) = pipewire::channel::channel::<EventMessage>();

        let pw_thread = thread::spawn(move || thread(
            client_info,
            main_sender,
            pw_receiver,
            event_sender,
            event_receiver
        ));

        let internal_api = Arc::new(InternalApi::new(pw_sender, main_receiver));
        let core_api = CoreApi::new(internal_api.clone());
        let node_api = NodeApi::new(internal_api.clone());
        let stream_api = StreamApi::new(internal_api.clone());

        let client = Self {
            name,
            socket_path,
            thread_handle: Some(pw_thread),
            internal_api,
            core_api,
            node_api,
            stream_api,
        };

        match client.wait_initialization() {
            Ok(_) => {}
            Err(value) => return Err(Error {
                description: format!("Initialization error: {}", value),
            })
        };
        match client.wait_post_initialization() {
            Ok(_) => {}
            Err(value) => return Err(Error {
                description: format!("Post initialization error: {}", value),
            }),
        };
        Ok(client)
    }

    fn wait_initialization(&self) -> Result<(), Error> {
        let timeout_duration = std::time::Duration::from_millis(10 * 1000);
        let response = self.internal_api.wait_response_with_timeout(timeout_duration);
        let response = match response {
            Ok(value) => value,
            Err(_) => {
                // Timeout is certainly due to missing session manager
                return self.core_api.check_session_manager_registered();
            }
        };
        match response {
            MessageResponse::Initialized => Ok(()),
            _ => Err(Error {
                description: format!("Received unexpected response: {:?}", response),
            }),
        }
    }

    fn wait_post_initialization(&self) -> Result<(), Error> {
        let mut settings_initialized = false;
        let mut default_audio_devices_initialized = false;
        let mut nodes_initialized = false;
        let timeout_duration = std::time::Duration::from_millis(1);
        self.core_api.check_session_manager_registered()?;
        match self.node_api.get_count() {
            Ok(value) => {
                if value == 0 {
                    return Err(Error {
                        description: "Zero node registered".to_string(),
                    })
                }
            }
            Err(value) => return Err(value),
        }
        self.core_api.get_settings_state()?;
        self.core_api.get_default_audio_nodes_state()?;
        self.node_api.get_states()?;
        let operation = move || {
            let response = self.internal_api.wait_response_with_timeout(timeout_duration);
            match response {
                Ok(value) => match value {
                    MessageResponse::SettingsState(state) => {
                        match state {
                            GlobalObjectState::Initialized => {
                                settings_initialized = true;
                            }
                            _ => {
                                self.core_api.get_settings_state()?;
                                return Err(Error {
                                    description: "Settings not yet initialized".to_string(),
                                })
                            }
                        };
                    },
                    MessageResponse::DefaultAudioNodesState(state) => {
                        match state {
                            GlobalObjectState::Initialized => {
                                default_audio_devices_initialized = true;
                            }
                            _ => {
                                self.core_api.get_default_audio_nodes_state()?;
                                return Err(Error {
                                    description: "Default audio nodes not yet initialized".to_string(),
                                })
                            }
                        }
                    },
                    MessageResponse::NodeStates(states) => {
                        let condition = states.iter()
                            .all(|state| *state == GlobalObjectState::Initialized);
                        match condition {
                            true => {
                                nodes_initialized = true;
                            },
                            false => {
                                self.node_api.get_states()?;
                                return Err(Error {
                                    description: "All nodes should be initialized at this point".to_string(),
                                })
                            }
                        };
                    }
                    MessageResponse::Error(value) => return Err(value),
                    _ => return Err(Error {
                        description: format!("Received unexpected response: {:?}", value),
                    }),
                }
                Err(_) => return Err(Error {
                    description: format!(
                        r"Timeout:
                            - settings: {}
                            - default audio nodes: {}
                            - nodes: {}",
                        settings_initialized,
                        default_audio_devices_initialized,
                        nodes_initialized
                    ),
                })
            };
            if settings_initialized == false || default_audio_devices_initialized == false || nodes_initialized == false {
                return Err(Error {
                    description: format!(
                        r"Conditions not yet initialized:
                            - settings: {}
                            - default audio nodes: {}
                            - nodes: {}",
                        settings_initialized,
                        default_audio_devices_initialized,
                        nodes_initialized
                    ),
                })
            }
            return Ok(());
        };
        let mut backoff = Backoff::new(
            30,
            std::time::Duration::from_millis(10),
            std::time::Duration::from_millis(100),
        );
        backoff.retry(operation)
    }

    pub(crate) fn internal(&self) -> Arc<InternalApi> {
        self.internal_api.clone()
    }

    pub fn core(&self) -> &CoreApi {
        &self.core_api
    }

    pub fn node(&self) -> &NodeApi {
        &self.node_api
    }

    pub fn stream(&self) -> &StreamApi {
        &self.stream_api
    }
}

impl Debug for PipewireClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "PipewireClient: {}", self.socket_path.to_str().unwrap())
    }
}

impl Drop for PipewireClient {
    fn drop(&mut self) {
        if self.internal_api.send_request_without_response(&MessageRequest::Quit).is_ok() {
            if let Some(thread_handle) = self.thread_handle.take() {
                if let Err(err) = thread_handle.join() {
                    panic!("Failed to join PipeWire thread: {:?}", err);
                }
            }
        } else {
            panic!("Failed to send Quit message to PipeWire thread.");
        }
    }
}
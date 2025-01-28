extern crate pipewire;

use std::thread;
use crate::client::api::{CoreApi, InternalApi, NodeApi, StreamApi};
use crate::client::channel::channels;
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
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::runtime::Runtime;

pub(super) static CLIENT_NAME_PREFIX: &str = "pipewire-client";
pub(super) static CLIENT_INDEX: AtomicU32 = AtomicU32::new(0);

pub struct PipewireClient {
    pub(crate) name: String,
    socket_path: PathBuf,
    thread_handle: Option<JoinHandle<()>>,
    timeout: Duration,
    internal_api: Arc<InternalApi>,
    core_api: CoreApi,
    node_api: NodeApi,
    stream_api: StreamApi,
}

impl PipewireClient {
    pub fn new(
        runtime: Arc<Runtime>,
        timeout: Duration,
    ) -> Result<Self, Error> {        
        let name = format!("{}-{}", CLIENT_NAME_PREFIX, CLIENT_INDEX.load(Ordering::SeqCst));
        CLIENT_INDEX.fetch_add(1, Ordering::SeqCst);

        let socket_path = PipewireClientSocketPath::from_env();

        let client_info = PipewireClientInfo {
            name: name.clone(),
            socket_location: socket_path.parent().unwrap().to_str().unwrap().to_string(),
            socket_name: socket_path.file_name().unwrap().to_str().unwrap().to_string(),
        };
        
        let (client_channel, server_channel) = channels(runtime.clone());
        let (event_sender, event_receiver) = pipewire::channel::channel::<EventMessage>();

        let pw_thread = thread::spawn(move || thread(
            client_info,
            server_channel,
            event_sender,
            event_receiver
        ));

        let internal_api = Arc::new(InternalApi::new(client_channel, timeout.clone()));
        let core_api = CoreApi::new(internal_api.clone());
        let node_api = NodeApi::new(internal_api.clone());
        let stream_api = StreamApi::new(internal_api.clone());

        let client = Self {
            name,
            socket_path,
            thread_handle: Some(pw_thread),
            timeout,
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
            Err(value) => {
                let global_messages = &client.internal_api.channel.global_messages;
                return Err(Error {
                    description: format!("Post initialization error: {}", value),
                })
            },
        };
        Ok(client)
    }

    fn wait_initialization(&self) -> Result<(), Error> {
        let response = self.internal_api.wait_response_with_timeout(self.timeout);
        let response = match response {
            Ok(value) => value,
            Err(value) => {
                // Timeout is certainly due to missing session manager
                // We need to check if that's the case. If session manager is running then we return
                // timeout error.
                return match self.core_api.check_session_manager_registered() {
                    Ok(_) => Err(value),
                    Err(value) => Err(value)
                };
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
        let mut default_audio_nodes_initialized = false;
        let mut nodes_initialized = false;
        self.core_api.check_session_manager_registered()?;
        match self.node_api.count() {
            Ok(value) => {
                if value == 0 {
                    return Err(Error {
                        description: "Zero node registered".to_string(),
                    })
                }
            }
            Err(value) => return Err(value),
        }
        let operation = move || {
            if settings_initialized == false {
                let settings_state = self.core_api.get_settings_state()?;
                if settings_state == GlobalObjectState::Initialized {
                    settings_initialized = true;
                }
            }
            if default_audio_nodes_initialized == false {
                let default_audio_nodes_state = self.core_api.get_default_audio_nodes_state()?;
                if default_audio_nodes_state == GlobalObjectState::Initialized {
                    default_audio_nodes_initialized = true;
                }
            }
            if nodes_initialized == false {
                let node_states = self.node_api.states()?;
                let condition = node_states.iter()
                    .all(|state| *state == GlobalObjectState::Initialized);
                if condition {
                    nodes_initialized = true;
                }
            }
            if settings_initialized == false || default_audio_nodes_initialized == false || nodes_initialized == false {
                return Err(Error {
                    description: format!(
                        r"Conditions not yet initialized:
                            - settings: {}
                            - default audio nodes: {}
                            - nodes: {}",
                        settings_initialized,
                        default_audio_nodes_initialized,
                        nodes_initialized
                    ),
                })
            }
            return Ok(());
        };
        let mut backoff = Backoff::constant(self.timeout.as_millis());
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
                thread_handle.join().unwrap();
            }
        } else {
            panic!("Failed to send Quit message to PipeWire thread.");
        }
    }
}
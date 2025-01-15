extern crate pipewire;

use crate::client::connection_string::{PipewireClientConnectionString, PipewireClientInfo};
use crate::client::handlers::thread;
use crate::error::Error;
use crate::info::{AudioStreamInfo, NodeInfo};
use crate::messages::{EventMessage, MessageRequest, MessageResponse, StreamCallback};
use crate::states::{DefaultAudioNodesState, GlobalId, GlobalObjectState, SettingsState};
use crate::utils::{Direction, Backoff};
use std::fmt::{Debug, Formatter};
use std::string::ToString;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;
use std::thread::JoinHandle;

pub(super) static CLIENT_NAME_PREFIX: &str = "cpal-client";
pub(super) static CLIENT_INDEX: AtomicU32 = AtomicU32::new(0);

pub struct PipewireClient {
    pub(crate) name: String,
    connection_string: String,
    sender: pipewire::channel::Sender<MessageRequest>,
    receiver: mpsc::Receiver<MessageResponse>,
    thread_handle: Option<JoinHandle<()>>,
}

impl PipewireClient {
    pub fn new() -> Result<Self, Error> {
        let name = format!("{}-{}", CLIENT_NAME_PREFIX, CLIENT_INDEX.load(Ordering::SeqCst));
        CLIENT_INDEX.fetch_add(1, Ordering::SeqCst);

        let connection_string = PipewireClientConnectionString::from_env();

        let client_info = PipewireClientInfo {
            name: name.clone(),
            connection_string: connection_string.clone(),
        };

        let (main_sender, main_receiver) = mpsc::channel();
        let (pw_sender, pw_receiver) = pipewire::channel::channel();
        let (event_sender, event_receiver) = pipewire::channel::channel::<EventMessage>();

        let pw_thread = thread::spawn(move || thread(
            client_info,
            main_sender,
            pw_receiver,
            event_sender,
            event_receiver
        ));

        let client = Self {
            name,
            connection_string,
            sender: pw_sender,
            receiver: main_receiver,
            thread_handle: Some(pw_thread),
        };

        match client.wait_initialization() {
            Ok(_) => {}
            Err(value) => return Err(value)
        };
        match client.wait_post_initialization() {
            Ok(_) => {}
            Err(value) => return Err(value),
        };
        Ok(client)
    }

    fn wait_initialization(&self) -> Result<(), Error> {
        let response = self.receiver.recv();
        let response = match response {
            Ok(value) => value,
            Err(value) => {
                return Err(Error {
                    description: format!(
                        "Failed during pipewire initialization: {:?}",
                        value
                    ),
                })
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
        #[cfg(debug_assertions)]
        let timeout_duration = std::time::Duration::from_secs(u64::MAX);
        #[cfg(not(debug_assertions))]
        let timeout_duration = std::time::Duration::from_millis(500);
        self.check_session_manager_registered()?;
        self.node_states()?;
        let operation = move || {
            let response = self.receiver.recv_timeout(timeout_duration);
            match response {
                Ok(value) => match value {
                    MessageResponse::SettingsState(state) => {
                        match state {
                            GlobalObjectState::Initialized => {
                                settings_initialized = true;
                            }
                            _ => return Err(Error {
                                description: "Settings not yet initialized".to_string(),
                            })
                        };
                    },
                    MessageResponse::DefaultAudioNodesState(state) => {
                        match state {
                            GlobalObjectState::Initialized => {
                                default_audio_devices_initialized = true;
                            }
                            _ => return Err(Error {
                                description: "Default audio nodes not yet initialized".to_string(),
                            })
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
                                self.node_states()?;
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
                Err(value) => return Err(Error {
                    description: format!("Failed during post initialization: {:?}", value),
                })
            };
            if settings_initialized == false || default_audio_devices_initialized == false || nodes_initialized == false {
                return Err(Error {
                    description: "Post initialization not yet finalized".to_string(),
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

    fn send_request(&self, request: &MessageRequest) -> Result<MessageResponse, Error> {
        let response = self.sender.send(request.clone());
        let response = match response {
            Ok(_) => self.receiver.recv(),
            Err(_) => return Err(Error {
                description: format!("Failed to send request: {:?}", request),
            }),
        };
        match response {
            Ok(value) => {
                match value {
                    MessageResponse::Error(value) => Err(value),
                    _ => Ok(value),
                }
            },
            Err(value) => Err(Error {
                description: format!(
                    "Failed to execute request ({:?}): {:?}",
                    request, value
                ),
            }),
        }
    }

    fn send_request_without_response(&self, request: &MessageRequest) -> Result<(), Error> {
        let response = self.sender.send(request.clone());
        match response {
            Ok(_) => Ok(()),
            Err(value) => Err(Error {
                description: format!(
                    "Failed to execute request ({:?}): {:?}",
                    request, value
                ),
            }),
        }
    }

    pub fn quit(&self) {
        let request = MessageRequest::Quit;
        self.send_request_without_response(&request).unwrap();
    }

    pub fn settings(&self) -> Result<SettingsState, Error> {
        let request = MessageRequest::Settings;
        let response = self.send_request(&request);
        match response {
            Ok(MessageResponse::Settings(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn default_audio_nodes(&self) -> Result<DefaultAudioNodesState, Error> {
        let request = MessageRequest::DefaultAudioNodes;
        let response = self.send_request(&request);
        match response {
            Ok(MessageResponse::DefaultAudioNodes(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn create_node(
        &self,
        name: String,
        description: String,
        nickname: String,
        direction: Direction,
        channels: u16,
    ) -> Result<(), Error> {
        let request = MessageRequest::CreateNode {
            name,
            description,
            nickname,
            direction,
            channels,
        };
        let response = self.send_request(&request);
        match response {
            Ok(MessageResponse::CreateNode {
                   id
               }) => {
                #[cfg(debug_assertions)]
                let timeout_duration = std::time::Duration::from_secs(u64::MAX);
                #[cfg(not(debug_assertions))]
                let timeout_duration = std::time::Duration::from_millis(500);
                self.node_state(&id)?;
                let operation = move || {
                    let response = self.receiver.recv_timeout(timeout_duration);
                    return match response {
                        Ok(value) => match value {
                            MessageResponse::NodeState(state) => {
                                match state == GlobalObjectState::Initialized {
                                    true => {
                                        Ok(())
                                    },
                                    false => {
                                        self.node_state(&id)?;
                                        Err(Error {
                                            description: "Created node should be initialized at this point".to_string(),
                                        })
                                    }
                                }
                            }
                            _ => Err(Error {
                                description: format!("Received unexpected response: {:?}", value),
                            }),
                        },
                        Err(value) => Err(Error {
                            description: format!("Failed during post initialization: {:?}", value),
                        })
                    };
                };
                let mut backoff = Backoff::new(
                    10,
                    std::time::Duration::from_millis(10),
                    std::time::Duration::from_millis(100),
                );
                backoff.retry(operation)
            },
            Ok(MessageResponse::Error(value)) => Err(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn enumerate_nodes(
        &self,
        direction: Direction,
    ) -> Result<Vec<NodeInfo>, Error> {
        let request = MessageRequest::EnumerateNodes(direction);
        let response = self.send_request(&request);
        match response {
            Ok(MessageResponse::EnumerateNodes(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn create_stream<F>(
        &self,
        node_id: u32,
        direction: Direction,
        format: AudioStreamInfo,
        callback: F,
    ) -> Result<String, Error>
    where
        F: FnMut(pipewire::buffer::Buffer) + Send + 'static
    {
        let request = MessageRequest::CreateStream {
            node_id: GlobalId::from(node_id),
            direction,
            format,
            callback: StreamCallback::from(callback),
        };
        let response = self.send_request(&request);
        match response {
            Ok(MessageResponse::CreateStream{name}) => Ok(name),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn delete_stream(
        &self,
        name: String
    ) -> Result<(), Error> {
        let request = MessageRequest::DeleteStream {
            name,
        };
        let response = self.send_request(&request);
        match response {
            Ok(MessageResponse::DeleteStream) => Ok(()),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value)
            }),
        }
    }

    pub fn connect_stream(
        &self,
        name: String
    ) -> Result<(), Error> {
        let request = MessageRequest::ConnectStream {
            name,
        };
        let response = self.send_request(&request);
        match response {
            Ok(MessageResponse::ConnectStream) => Ok(()),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn disconnect_stream(
        &self,
        name: String
    ) -> Result<(), Error> {
        let request = MessageRequest::DisconnectStream {
            name,
        };
        let response = self.send_request(&request);
        match response {
            Ok(MessageResponse::DisconnectStream) => Ok(()),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    // Internal requests
    pub(super) fn check_session_manager_registered(&self) -> Result<(), Error> {
        let request = MessageRequest::CheckSessionManagerRegistered;
        self.send_request_without_response(&request)
    }

    pub(super) fn node_state(
        &self,
        id: &GlobalId,
    ) -> Result<(), Error> {
        let request = MessageRequest::NodeState(id.clone());
        self.send_request_without_response(&request)
    }

    pub(super) fn node_states(
        &self,
    ) -> Result<(), Error> {
        let request = MessageRequest::NodeStates;
        self.send_request_without_response(&request)
    }
}

impl Debug for PipewireClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "PipewireClient: {}", self.connection_string)
    }
}

impl Drop for PipewireClient {
    fn drop(&mut self) {
        if self.sender.send(MessageRequest::Quit).is_ok() {
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
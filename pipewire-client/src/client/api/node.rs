use crate::client::api::internal::InternalApi;
use crate::error::Error;
use crate::messages::{MessageRequest, MessageResponse};
use crate::states::{GlobalId, GlobalObjectState};
use crate::utils::Backoff;
use crate::{Direction, NodeInfo};
use std::sync::Arc;

pub struct NodeApi {
    api: Arc<InternalApi>
}

impl NodeApi {
    pub(crate) fn new(api: Arc<InternalApi>) -> Self {
        NodeApi {
            api,
        }
    }

    pub(crate) fn get_state(
        &self,
        id: &GlobalId,
    ) -> Result<(), Error> {
        let request = MessageRequest::NodeState(id.clone());
        self.api.send_request_without_response(&request)
    }

    pub(crate) fn get_states(
        &self,
    ) -> Result<(), Error> {
        let request = MessageRequest::NodeStates;
        self.api.send_request_without_response(&request)
    }

    pub(crate) fn get_count(
        &self,
    ) -> Result<u32, Error> {
        let request = MessageRequest::NodeCount;
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::NodeCount(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn create(
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
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::CreateNode {
                   id
               }) => {
                #[cfg(debug_assertions)]
                let timeout_duration = std::time::Duration::from_secs(u64::MAX);
                #[cfg(not(debug_assertions))]
                let timeout_duration = std::time::Duration::from_millis(500);
                self.get_state(&id)?;
                let operation = move || {
                    let response = self.api.wait_response_with_timeout(timeout_duration);
                    return match response {
                        Ok(value) => match value {
                            MessageResponse::NodeState(state) => {
                                match state == GlobalObjectState::Initialized {
                                    true => {
                                        Ok(())
                                    },
                                    false => {
                                        self.get_state(&id)?;
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

    pub fn enumerate(
        &self,
        direction: Direction,
    ) -> Result<Vec<NodeInfo>, Error> {
        let request = MessageRequest::EnumerateNodes(direction);
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::EnumerateNodes(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }
}
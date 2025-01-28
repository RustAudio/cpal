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

    pub(crate) fn state(
        &self,
        id: &GlobalId,
    ) -> Result<GlobalObjectState, Error> {
        let request = MessageRequest::NodeState(id.clone());
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::NodeState(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub(crate) fn states(
        &self,
    ) -> Result<Vec<GlobalObjectState>, Error> {
        let request = MessageRequest::NodeStates;
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::NodeStates(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub(crate) fn count(
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

    pub fn get(
        &self,
        name: String,
        direction: Direction,
    ) -> Result<NodeInfo, Error> {
        let request = MessageRequest::GetNode {
            name,
            direction,
        };
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::GetNode(value)) => Ok(value),
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
            Ok(MessageResponse::CreateNode(id)) => {
                let operation = move || {
                    let state = self.state(&id)?;
                    return if state == GlobalObjectState::Initialized {
                        Ok(())
                    } else {
                        Err(Error {
                            description: "Created node not yet initialized".to_string(),
                        })
                    }
                };
                let mut backoff = Backoff::constant(self.api.timeout.as_millis());
                backoff.retry(operation)
            },
            Ok(MessageResponse::Error(value)) => Err(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn delete(&self, id: u32) -> Result<(), Error> {
        let request = MessageRequest::DeleteNode(GlobalId::from(id));
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::DeleteNode) => Ok(()),
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
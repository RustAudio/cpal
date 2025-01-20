use crate::client::api::internal::InternalApi;
use crate::error::Error;
use crate::listeners::ListenerControlFlow;
use crate::messages::{MessageRequest, MessageResponse, StreamCallback};
use crate::states::GlobalId;
use crate::{AudioStreamInfo, Direction};
use std::sync::Arc;

pub struct StreamApi {
    api: Arc<InternalApi>,
}

impl StreamApi {
    pub(crate) fn new(api: Arc<InternalApi>) -> Self {
        StreamApi {
            api,
        }
    }

    pub fn create<F>(
        &self,
        node_id: u32,
        direction: Direction,
        format: AudioStreamInfo,
        callback: F,
    ) -> Result<String, Error>
    where
        F: FnMut(&mut ListenerControlFlow, pipewire::buffer::Buffer) + Send + 'static
    {
        let request = MessageRequest::CreateStream {
            node_id: GlobalId::from(node_id),
            direction,
            format,
            callback: StreamCallback::from(callback),
        };
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::CreateStream{name}) => Ok(name),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn delete(
        &self,
        name: String
    ) -> Result<(), Error> {
        let request = MessageRequest::DeleteStream {
            name,
        };
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::DeleteStream) => Ok(()),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value)
            }),
        }
    }

    pub fn connect(
        &self,
        name: String
    ) -> Result<(), Error> {
        let request = MessageRequest::ConnectStream {
            name,
        };
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::ConnectStream) => Ok(()),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn disconnect(
        &self,
        name: String
    ) -> Result<(), Error> {
        let request = MessageRequest::DisconnectStream {
            name,
        };
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::DisconnectStream) => Ok(()),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }
}
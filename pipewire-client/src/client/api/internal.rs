use crate::error::Error;
use crate::messages::{MessageRequest, MessageResponse};
use crossbeam_channel::{RecvError, RecvTimeoutError};
use std::time::Duration;

pub(crate) struct InternalApi {
    sender: pipewire::channel::Sender<MessageRequest>,
    receiver: crossbeam_channel::Receiver<MessageResponse>,
}

impl InternalApi {
    pub(crate) fn new(
        sender: pipewire::channel::Sender<MessageRequest>,
        receiver: crossbeam_channel::Receiver<MessageResponse>,
    ) -> Self {
        InternalApi {
            sender,
            receiver,
        }
    }

    pub(crate) fn wait_response(&self) -> Result<MessageResponse, RecvError> {
        self.receiver.recv()
    }

    pub(crate) fn wait_response_with_timeout(&self, timeout: Duration) -> Result<MessageResponse, RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }

    pub(crate) fn send_request(&self, request: &MessageRequest) -> Result<MessageResponse, Error> {
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

    pub(crate) fn send_request_without_response(&self, request: &MessageRequest) -> Result<(), Error> {
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
}
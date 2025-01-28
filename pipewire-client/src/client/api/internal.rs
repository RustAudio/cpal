use crate::client::channel::ClientChannel;
use crate::error::Error;
use crate::messages::{MessageRequest, MessageResponse};
use std::time::Duration;

pub(crate) struct InternalApi {
    pub(crate) channel: ClientChannel<MessageRequest, MessageResponse>,
    pub(crate) timeout: Duration
}

impl InternalApi {
    pub(crate) fn new(
        channel: ClientChannel<MessageRequest, MessageResponse>,
        timeout: Duration
    ) -> Self {
        InternalApi {
            channel,
            timeout,
        }
    }

    pub(crate) fn wait_response_with_timeout(&self, timeout: Duration) -> Result<MessageResponse, Error> {
        self.channel.receive_timeout(timeout)
    }

    pub(crate) fn send_request(&self, request: &MessageRequest) -> Result<MessageResponse, Error> {
        let response = self.channel.send(request.clone());
        match response {
            Ok(value) => {
                match value {
                    MessageResponse::Error(value) => Err(value),
                    _ => Ok(value)
                }
            }
            Err(value) => Err(value)
        }
    }

    pub(crate) fn send_request_without_response(&self, request: &MessageRequest) -> Result<(), Error> {
        self.channel.fire(request.clone()).map(move |_| ())
    }
}
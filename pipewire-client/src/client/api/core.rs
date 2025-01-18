use crate::client::api::internal::InternalApi;
use crate::error::Error;
use crate::messages::{MessageRequest, MessageResponse};
use crate::states::{DefaultAudioNodesState, SettingsState};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub struct CoreApi {
    api: Arc<InternalApi>,
}

impl CoreApi {
    pub(crate) fn new(api: Arc<InternalApi>) -> Self {
        CoreApi {
            api,
        }
    }

    pub(crate) fn check_session_manager_registered(&self) -> Result<(), Error> {
        let request = MessageRequest::CheckSessionManagerRegistered;
        self.api.send_request_without_response(&request)
    }

    pub fn quit(&self) {
        let request = MessageRequest::Quit;
        self.api.send_request_without_response(&request).unwrap();
    }

    pub fn get_settings(&self) -> Result<SettingsState, Error> {
        let request = MessageRequest::Settings;
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::Settings(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }

    pub fn get_default_audio_nodes(&self) -> Result<DefaultAudioNodesState, Error> {
        let request = MessageRequest::DefaultAudioNodes;
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::DefaultAudioNodes(value)) => Ok(value),
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }
}
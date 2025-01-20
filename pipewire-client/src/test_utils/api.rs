use crate::client::CoreApi;
use crate::error::Error;
use crate::messages::{MessageRequest, MessageResponse};
use crate::states::{MetadataState, NodeState, StreamState};
use crate::utils::PipewireCoreSync;
use std::any::TypeId;
use std::collections::HashMap;

impl CoreApi {
    pub(crate) fn get_listeners(&self) -> Result<HashMap<TypeId, HashMap<String, Vec<String>>>, Error> {
        let request = MessageRequest::Listeners;
        let response = self.api.send_request(&request);
        match response {
            Ok(MessageResponse::Listeners {
                   core,
                   metadata,
                   nodes,
                   streams
               }) => {
                let mut map = HashMap::new();
                map.insert(TypeId::of::<PipewireCoreSync>(), core);
                map.insert(TypeId::of::<MetadataState>(), metadata);
                map.insert(TypeId::of::<NodeState>(), nodes);
                map.insert(TypeId::of::<StreamState>(), streams);
                Ok(map)
            },
            Err(value) => Err(value),
            Ok(value) => Err(Error {
                description: format!("Received unexpected response: {:?}", value),
            }),
        }
    }
}
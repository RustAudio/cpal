use crate::constants::{METADATA_NAME_PROPERTY_VALUE_DEFAULT, METADATA_NAME_PROPERTY_VALUE_SETTINGS};
use crate::error::Error;
use crate::messages::{EventMessage, MessageRequest, MessageResponse};
use crate::states::{DefaultAudioNodesState, GlobalId, GlobalState, SettingsState};
use pipewire_spa_utils::audio::raw::AudioInfoRaw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use crate::client::channel::ServerChannel;

pub(super) fn event_handler(
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
) -> impl Fn(EventMessage) + 'static
{    
    move |event_message: EventMessage| match event_message {
        EventMessage::SetMetadataListeners { id } => handle_set_metadata_listeners(
            id,
            state.clone(),
            server_channel.clone(),
        ),
        EventMessage::RemoveNode { id } => handle_remove_node(
            id, 
            state.clone(),
            server_channel.clone()
        ),
        EventMessage::SetNodePropertiesListener { id } => handle_set_node_properties_listener(
            id, 
            state.clone(),
            server_channel.clone(), 
            event_sender.clone()
        ),
        EventMessage::SetNodeFormatListener { id } => handle_set_node_format_listener(
            id, 
            state.clone(),
            server_channel.clone(), 
            event_sender.clone()
        ),
        EventMessage::SetNodeProperties { 
            id, 
            properties 
        } => handle_set_node_properties(
            id, 
            properties, 
            state.clone(),
            server_channel.clone()
        ),
        EventMessage::SetNodeFormat { id, format } => handle_set_node_format(
            id, 
            format, 
            state.clone(),
            server_channel.clone()
        ),
    }
}

fn handle_set_metadata_listeners(
    id: GlobalId,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
) 
{
    let listener_state = state.clone();
    let mut state = state.lock().unwrap();
    let metadata = match state.get_metadata_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            server_channel
                .fire(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let server_channel = server_channel.clone();
    match metadata.name.as_str() {
        METADATA_NAME_PROPERTY_VALUE_SETTINGS => {
            metadata.add_property_listener(
                SettingsState::listener(listener_state)
            )
        },
        METADATA_NAME_PROPERTY_VALUE_DEFAULT => {
            metadata.add_property_listener(
                DefaultAudioNodesState::listener(listener_state)
            )
        },
        _ => {
            server_channel
                .fire(MessageResponse::Error(Error {
                    description: format!("Unexpected metadata with name: {}", metadata.name)
                }))
                .unwrap();
        }
    };
}
fn handle_remove_node(
    id: GlobalId,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
) 
{
    let mut state = state.lock().unwrap();
    let _ = match state.get_node(&id) {
        Ok(_) => {},
        Err(value) => {
            server_channel
                .fire(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    state.remove(&id);
}
fn handle_set_node_properties_listener(
    id: GlobalId,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
) 
{
    let mut state = state.lock().unwrap();
    let node = match state.get_node_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            server_channel
                .fire(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let event_sender = event_sender.clone();
    node.add_properties_listener(
        move |control_flow, properties| {
            // "object.register" property when set to "false", indicate we should not
            // register this object
            // Some bluez nodes don't have sample rate information in their
            // EnumFormat object. We delete those nodes since parsing node audio format
            // imply to retrieve:
            //   - Media type
            //   - Media subtype
            //   - Sample format
            //   - Sample rate
            //   - Channels
            //   - Channels position
            // Lets see in the future if node with no "object.register: false" property
            // and with incorrect EnumFormat object occur.
            if properties.get("object.register").is_some_and(move |value| value == "false") {
                event_sender
                    .send(EventMessage::RemoveNode {
                        id: id.clone(),
                    })
                    .unwrap();
            }
            else {
                event_sender
                    .send(EventMessage::SetNodeProperties {
                        id: id.clone(),
                        properties,
                    })
                    .unwrap();
                event_sender
                    .send(EventMessage::SetNodeFormatListener {
                        id: id.clone(),
                    })
                    .unwrap();
            }
            control_flow.release();
        }
    );
}
fn handle_set_node_format_listener(
    id: GlobalId,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
) 
{
    let mut state = state.lock().unwrap();
    let node = match state.get_node_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            server_channel
                .fire(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let server_channel = server_channel.clone();
    let event_sender = event_sender.clone();
    node.add_format_listener(
        move |control_flow, format| {
            match format {
                Ok(value) => {
                    event_sender
                        .send(EventMessage::SetNodeFormat {
                            id,
                            format: value,
                        })
                        .unwrap();
                }
                Err(value) => {
                    server_channel
                        .fire(MessageResponse::Error(value))
                        .unwrap();
                }
            };
            control_flow.release();
        }
    )
}
fn handle_set_node_properties(
    id: GlobalId,
    properties: HashMap<String, String>,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
) 
{
    let mut state = state.lock().unwrap();
    let node = match state.get_node_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            server_channel
                .fire(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    node.set_properties(properties);
}
fn handle_set_node_format(
    id: GlobalId,
    format: AudioInfoRaw,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
) 
{
    let mut state = state.lock().unwrap();
    let node = match state.get_node_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            server_channel
                .fire(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    node.set_format(format);
}
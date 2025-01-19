use crate::constants::{METADATA_NAME_PROPERTY_VALUE_DEFAULT, METADATA_NAME_PROPERTY_VALUE_SETTINGS};
use crate::error::Error;
use crate::listeners::ListenerTriggerPolicy;
use crate::messages::{EventMessage, MessageResponse};
use crate::states::{DefaultAudioNodesState, GlobalId, GlobalState, SettingsState};
use pipewire_spa_utils::audio::raw::AudioInfoRaw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub(super) fn event_handler(
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
) -> impl Fn(EventMessage) + 'static
{    
    move |event_message: EventMessage| match event_message {
        EventMessage::SetMetadataListeners { id } => handle_set_metadata_listeners(
            id,
            state.clone(),
            main_sender.clone(),
        ),
        EventMessage::RemoveNode { id } => handle_remove_node(
            id, 
            state.clone(), 
            main_sender.clone()
        ),
        EventMessage::SetNodePropertiesListener { id } => handle_set_node_properties_listener(
            id, 
            state.clone(), 
            main_sender.clone(), 
            event_sender.clone()
        ),
        EventMessage::SetNodeFormatListener { id } => handle_set_node_format_listener(
            id, 
            state.clone(), 
            main_sender.clone(), 
            event_sender.clone()
        ),
        EventMessage::SetNodeProperties { 
            id, 
            properties 
        } => handle_set_node_properties(
            id, 
            properties, 
            state.clone(), 
            main_sender.clone()
        ),
        EventMessage::SetNodeFormat { id, format } => handle_set_node_format(
            id, 
            format, 
            state.clone(), 
            main_sender.clone()
        ),
    }
}

fn handle_set_metadata_listeners(
    id: GlobalId,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let listener_state = state.clone();
    let mut state = state.borrow_mut();
    let metadata = match state.get_metadata_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let main_sender = main_sender.clone();
    match metadata.name.as_str() {
        METADATA_NAME_PROPERTY_VALUE_SETTINGS => {
            metadata.add_property_listener(
                ListenerTriggerPolicy::Keep,
                SettingsState::listener(listener_state)
            )
        },
        METADATA_NAME_PROPERTY_VALUE_DEFAULT => {
            metadata.add_property_listener(
                ListenerTriggerPolicy::Keep,
                DefaultAudioNodesState::listener(listener_state)
            )
        },
        _ => {
            main_sender
                .send(MessageResponse::Error(Error {
                    description: format!("Unexpected metadata with name: {}", metadata.name)
                }))
                .unwrap();
        }
    };
}
fn handle_remove_node(
    id: GlobalId,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let mut state = state.borrow_mut();
    let _ = match state.get_node(&id) {
        Ok(_) => {},
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    state.remove(&id);
}
fn handle_set_node_properties_listener(
    id: GlobalId,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
) 
{
    let mut state = state.borrow_mut();
    let node = match state.get_node_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let event_sender = event_sender.clone();
    node.add_properties_listener(
        ListenerTriggerPolicy::Keep,
        move |properties| {
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
        }
    );
}
fn handle_set_node_format_listener(
    id: GlobalId,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
) 
{
    let mut state = state.borrow_mut();
    let node = match state.get_node_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let main_sender = main_sender.clone();
    let event_sender = event_sender.clone();
    node.add_format_listener(
        ListenerTriggerPolicy::Keep,
        move |format| {
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
                    main_sender.send(MessageResponse::Error(value)).unwrap();
                }
            }
        }
    )
}
fn handle_set_node_properties(
    id: GlobalId,
    properties: HashMap<String, String>,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let mut state = state.borrow_mut();
    let node = match state.get_node_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    node.set_properties(properties);
}
fn handle_set_node_format(
    id: GlobalId,
    format: AudioInfoRaw,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let mut state = state.borrow_mut();
    let node = match state.get_node_mut(&id) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    node.set_format(format)
}
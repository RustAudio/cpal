use crate::client::channel::ServerChannel;
use crate::constants::{APPLICATION_NAME_PROPERTY_KEY, APPLICATION_NAME_PROPERTY_VALUE_PIPEWIRE_MEDIA_SESSION, APPLICATION_NAME_PROPERTY_VALUE_WIRE_PLUMBER, MEDIA_CLASS_PROPERTY_KEY, MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SINK, MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SOURCE, METADATA_NAME_PROPERTY_KEY, METADATA_NAME_PROPERTY_VALUE_DEFAULT, METADATA_NAME_PROPERTY_VALUE_SETTINGS};
use crate::messages::{EventMessage, MessageRequest, MessageResponse};
use crate::states::{ClientState, GlobalId, GlobalObjectState, GlobalState, MetadataState, NodeState};
use pipewire::registry::GlobalObject;
use pipewire::spa;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use pipewire_common::utils::dict_ref_to_hashmap;

pub(super) fn registry_global_handler(
    state: Arc<Mutex<GlobalState>>,
    registry: Rc<pipewire::registry::Registry>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
) -> impl Fn(&GlobalObject<&spa::utils::dict::DictRef>) + 'static
{
    move |global: &GlobalObject<&spa::utils::dict::DictRef>| match global.type_ {
        pipewire::types::ObjectType::Client => handle_client(
            global, 
            state.clone(),
            server_channel.clone()
        ),
        pipewire::types::ObjectType::Metadata => handle_metadata(
            global, 
            state.clone(), 
            registry.clone(),
            server_channel.clone(), 
            event_sender.clone()
        ),
        pipewire::types::ObjectType::Node => handle_node(
            global,
            state.clone(),
            registry.clone(),
            server_channel.clone(),
            event_sender.clone()
        ),
        pipewire::types::ObjectType::Port => handle_port(
            global,
            state.clone(),
            registry.clone(),
            server_channel.clone(),
            event_sender.clone()
        ),
        pipewire::types::ObjectType::Link => handle_link(
            global,
            state.clone(),
            registry.clone(),
            server_channel.clone(),
            event_sender.clone()
        ),
        _ => {}
    }
}

fn handle_client(
    global: &GlobalObject<&spa::utils::dict::DictRef>,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
) 
{
    if global.props.is_none() {
        return;
    }
    let properties = global.props.unwrap();
    let client =
        match properties.get(APPLICATION_NAME_PROPERTY_KEY) {
            Some(APPLICATION_NAME_PROPERTY_VALUE_WIRE_PLUMBER) => {
                ClientState::new(
                    APPLICATION_NAME_PROPERTY_VALUE_WIRE_PLUMBER.to_string()
                )
            }
            Some(APPLICATION_NAME_PROPERTY_VALUE_PIPEWIRE_MEDIA_SESSION) => {
                ClientState::new(
                    APPLICATION_NAME_PROPERTY_VALUE_PIPEWIRE_MEDIA_SESSION.to_string()
                )
            }
            _ => return,
        };
    let mut state = state.lock().unwrap();
    if let Err(value) = state.insert_client(global.id.into(), client) {
        server_channel
            .fire(MessageResponse::Error(value))
            .unwrap();
        return;
    };
}

fn handle_metadata(
    global: &GlobalObject<&spa::utils::dict::DictRef>,
    state: Arc<Mutex<GlobalState>>,
    registry: Rc<pipewire::registry::Registry>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
) 
{
    if global.props.is_none() {
        return;
    }
    let properties = global.props.unwrap();
    let metadata =
        match properties.get(METADATA_NAME_PROPERTY_KEY) {
            Some(METADATA_NAME_PROPERTY_VALUE_SETTINGS)
            | Some(METADATA_NAME_PROPERTY_VALUE_DEFAULT) => {
                let metadata = registry.bind(global).unwrap();
                MetadataState::new(
                    metadata,
                    properties.get(METADATA_NAME_PROPERTY_KEY).unwrap().to_string(),
                )
            }
            _ => return,
        };
    let mut state = state.lock().unwrap();
    if let Err(value) = state.insert_metadata(global.id.into(), metadata) {
        server_channel
            .fire(MessageResponse::Error(value))
            .unwrap();
        return;
    };
    let metadata = state.get_metadata(&global.id.into()).unwrap();
    add_metadata_listeners(
        global.id.into(),
        &metadata,
        &event_sender
    );
}

fn handle_node(
    global: &GlobalObject<&spa::utils::dict::DictRef>,
    state: Arc<Mutex<GlobalState>>,
    registry: Rc<pipewire::registry::Registry>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
)
{
    if global.props.is_none() {
        return;
    }
    let properties = global.props.unwrap();
    let mut node = match properties.get(MEDIA_CLASS_PROPERTY_KEY) {
        Some(MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SOURCE)
        | Some(MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SINK) => {
            let node: pipewire::node::Node = registry.bind(global).unwrap();
            NodeState::new(node)
        }
        _ => return,
    };
    node.set_properties(dict_ref_to_hashmap(properties));
    let mut state = state.lock().unwrap();
    if let Err(value) = state.insert_node(global.id.into(), node) {
        server_channel
            .fire(MessageResponse::Error(value))
            .unwrap();
        return;
    };
    let node = state.get_node(&global.id.into()).unwrap();
    add_node_listeners(
        global.id.into(),
        &node,
        &event_sender
    );
}

fn handle_port(
    global: &GlobalObject<&spa::utils::dict::DictRef>,
    state: Arc<Mutex<GlobalState>>,
    registry: Rc<pipewire::registry::Registry>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
)
{
    if global.props.is_none() {

    }
    let properties = global.props.unwrap();
    // debug_dict_ref(properties);

    let port: pipewire::port::Port = registry.bind(global).unwrap();

    // let node = match properties.get(MEDIA_CLASS_PROPERTY_KEY) {
    //     Some(MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SOURCE)
    //     | Some(MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SINK) => {
    //         let node: pipewire::node::Node = registry.bind(global).unwrap();
    //         NodeState::new(node)
    //     }
    //     _ => return,
    // };
    // let mut state = state.borrow_mut();
    // if let Err(value) = state.insert_node(global.id.into(), node) {
    //     main_sender
    //         .send(MessageResponse::Error(value))
    //         .unwrap();
    //     return;
    // };
    // let node = state.get_node(&global.id.into()).unwrap();
    // add_node_listeners(
    //     global.id.into(),
    //     &node,
    //     &event_sender
    // );
}

fn handle_link(
    global: &GlobalObject<&spa::utils::dict::DictRef>,
    state: Arc<Mutex<GlobalState>>,
    registry: Rc<pipewire::registry::Registry>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
    event_sender: pipewire::channel::Sender<EventMessage>,
)
{
    if global.props.is_none() {

    }
    let properties = global.props.unwrap();
    // debug_dict_ref(properties);
    
    let link: pipewire::link::Link = registry.bind(global).unwrap();
    // link.
    
    // let node = match properties.get(MEDIA_CLASS_PROPERTY_KEY) {
    //     Some(MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SOURCE)
    //     | Some(MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SINK) => {
    //         let node: pipewire::node::Node = registry.bind(global).unwrap();
    //         NodeState::new(node)
    //     }
    //     _ => return,
    // };
    // let mut state = state.borrow_mut();
    // if let Err(value) = state.insert_node(global.id.into(), node) {
    //     main_sender
    //         .send(MessageResponse::Error(value))
    //         .unwrap();
    //     return;
    // };
    // let node = state.get_node(&global.id.into()).unwrap();
    // add_node_listeners(
    //     global.id.into(),
    //     &node,
    //     &event_sender
    // );
}

fn add_metadata_listeners(
    id: GlobalId,
    metadata: &MetadataState,
    event_sender: &pipewire::channel::Sender<EventMessage>
) {
    if *metadata.state.borrow() != GlobalObjectState::Pending {
        return;
    }
    let id = id.clone();
    event_sender
        .send(EventMessage::SetMetadataListeners {
            id,
        })
        .unwrap()
}

fn add_node_listeners(
    id: GlobalId,
    node: &NodeState,
    event_sender: &pipewire::channel::Sender<EventMessage>
) {
    if node.state() != GlobalObjectState::Pending {
        return;
    }
    let id = id.clone();
    event_sender
        .send(EventMessage::SetNodePropertiesListener {
            id,
        })
        .unwrap()
}
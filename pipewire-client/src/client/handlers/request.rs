use std::cell::RefCell;
use std::rc::Rc;
use pipewire::proxy::ProxyT;
use crate::constants::*;
use crate::{AudioStreamInfo, Direction, NodeInfo};
use crate::error::Error;
use crate::listeners::ListenerTriggerPolicy;
use crate::messages::{MessageRequest, MessageResponse, StreamCallback};
use crate::states::{GlobalId, GlobalObjectState, GlobalState, OrphanState, StreamState};
use crate::utils::PipewireCoreSync;

pub(super) fn request_handler(
    core: Rc<pipewire::core::Core>,
    core_sync: Rc<PipewireCoreSync>,
    main_loop: pipewire::main_loop::MainLoop,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) -> impl Fn(MessageRequest) + 'static
{
    move |message_request: MessageRequest| match message_request {
        MessageRequest::Quit => main_loop.quit(),
        MessageRequest::Settings => {
            handle_settings(
                state.clone(),
                main_sender.clone(),
            )
        }
        MessageRequest::DefaultAudioNodes => {
            handle_default_audio_nodes(
                state.clone(), 
                main_sender.clone()
            )
        },
        MessageRequest::CreateNode {
            name,
            description,
            nickname,
            direction,
            channels,
        } => {
            handle_create_node(
                name,
                description,
                nickname,
                direction,
                channels,
                core.clone(),
                core_sync.clone(),
                state.clone(),
                main_sender.clone(),
            )
        }
        MessageRequest::EnumerateNodes(direction) => {
            handle_enumerate_node(
                direction,
                state.clone(),
                main_sender.clone(),
            )
        },
        MessageRequest::CreateStream {
            node_id,
            direction,
            format,
            callback
        } => {
            handle_create_stream(
                node_id,
                direction,
                format,
                callback,
                core.clone(),
                state.clone(),
                main_sender.clone(),
            )
        }
        MessageRequest::DeleteStream { name } => {
            handle_delete_stream(
                name,
                state.clone(),
                main_sender.clone()
            )
        }
        MessageRequest::ConnectStream { name } => {
            handle_connect_stream(
                name,
                state.clone(),
                main_sender.clone()
            )
        }
        MessageRequest::DisconnectStream { name } => {
            handle_disconnect_stream(
                name,
                state.clone(),
                main_sender.clone()
            )
        }
        // Internal requests
        MessageRequest::CheckSessionManagerRegistered => {
            handle_check_session_manager_registered(
                state.clone(),
                main_sender.clone()
            )
        }
        MessageRequest::NodeState(id) => {
            handle_node_state(
                id,
                state.clone(),
                main_sender.clone()
            )
        }
        MessageRequest::NodeStates => {
            handle_node_states(
                state.clone(),
                main_sender.clone()
            )
        }
    }
}

fn handle_settings(
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let state = state.borrow();
    let settings = state.get_settings();
    main_sender.send(MessageResponse::Settings(settings)).unwrap();
}
fn handle_default_audio_nodes(
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let state = state.borrow();
    let default_audio_devices = state.get_default_audio_nodes();
    main_sender.send(MessageResponse::DefaultAudioNodes(default_audio_devices)).unwrap();
}
fn handle_create_node(
    name: String,
    description: String,
    nickname: String,
    direction: Direction,
    channels: u16,
    core: Rc<pipewire::core::Core>,
    core_sync: Rc<PipewireCoreSync>,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let default_audio_position = format!(
        "[ {} ]",
        (1..=channels + 1)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let properties = &pipewire::properties::properties! {
        *pipewire::keys::FACTORY_NAME => "support.null-audio-sink",
        *pipewire::keys::NODE_NAME => name.clone(),
        *pipewire::keys::NODE_DESCRIPTION => description.clone(),
        *pipewire::keys::NODE_NICK => nickname.clone(),
        *pipewire::keys::MEDIA_CLASS => match direction {
            Direction::Input => MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SOURCE,
            Direction::Output => MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SINK,
        },
        *pipewire::keys::OBJECT_LINGER => "false",
        *pipewire::keys::AUDIO_CHANNELS => channels.to_string(),
        MONITOR_CHANNEL_VOLUMES_PROPERTY_KEY => "true",
        MONITOR_PASSTHROUGH_PROPERTY_KEY => "true",
        AUDIO_POSITION_PROPERTY_KEY => match channels {
            1 => "[ MONO ]",
            2 => "[ FL FR ]", // 2.0
            3 => "[ FL FR LFE ]", // 2.1
            4 => "[ FL FR RL RR ]", // 4.0
            5 => "[ FL FR FC RL RR ]", // 5.0
            6 => "[ FL FR FC RL RR LFE ]", // 5.1
            7 => "[ FL FR FC RL RR SL SR ]", // 7.0
            8 => "[ FL FR FC RL RR SL SR LFE ]", // 7.1
            _ => default_audio_position.as_str(),
        }
    };
    let node: pipewire::node::Node = match core
        .create_object("adapter", properties)
        .map_err(move |error| {
            Error {
                description: error.to_string(),
            }
        }) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let core_sync = core_sync.clone();
    let listener_main_sender = main_sender.clone();
    let listener_state = state.clone();
    core_sync.register(
        false,
        PIPEWIRE_CORE_SYNC_CREATE_DEVICE_SEQ,
        move || {
            let state = listener_state.borrow();
            let nodes = match state.get_nodes() {
                Ok(value) => value,
                Err(value) => {
                    listener_main_sender
                        .send(MessageResponse::Error(value))
                        .unwrap();
                    return;
                }
            };
            let node = nodes.iter()
                .find(move |(_, node)| {
                    node.state() == GlobalObjectState::Pending
                });
            if let None = node {
                listener_main_sender
                    .send(MessageResponse::Error(Error {
                        description: "Created node not found".to_string(),
                    }))
                    .unwrap();
                return;
            };
            let node_id = node.unwrap().0;
            listener_main_sender
                .send(MessageResponse::CreateNode {
                    id: (*node_id).clone(),
                })
                .unwrap();
        }
    );
    let mut state = state.borrow_mut();
    // We need to store created node object as orphan since it had not been
    // registered by server at this point (does not have an id yet).
    //
    // When a proxy object is dropped its send a server request to remove it on server
    // side, then the server ask clients to remove proxy object on their side.
    //
    // The server will send a global object (through registry global object event
    // listener) later, represented by a new proxy object instance that we can store
    // as a NodeState.
    // OrphanState object define "removed" listener from Proxy to ensure our orphan
    // proxy object is removed when proper NodeState object is retrieved from server
    let orphan = OrphanState::new(node.upcast());
    state.insert_orphan(orphan);
}
fn handle_enumerate_node(
    direction: Direction,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let state = state.borrow();
    let default_audio_nodes = state.get_default_audio_nodes();
    let default_audio_node = match direction {
        Direction::Input => default_audio_nodes.source.clone(),
        Direction::Output => default_audio_nodes.sink.clone()
    };
    let filter_value = match direction {
        Direction::Input => MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SOURCE,
        Direction::Output => MEDIA_CLASS_PROPERTY_VALUE_AUDIO_SINK,
    };
    let nodes = match state.get_nodes() {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let nodes: Vec<NodeInfo> = nodes
        .iter()
        .filter_map(|(id, node)| {
            let properties = node.properties();
            let format = node.format().unwrap();
            if properties.iter().any(|(_, v)| v == filter_value) {
                Some((id, properties, format))
            } else {
                None
            }
        })
        .map(|(id, properties, format)| {
            let name = properties.get(*pipewire::keys::NODE_NAME).unwrap().clone();
            let description = properties
                .get(*pipewire::keys::NODE_DESCRIPTION)
                .unwrap()
                .clone();
            let nickname = match properties.contains_key(*pipewire::keys::NODE_NICK) {
                true => properties.get(*pipewire::keys::NODE_NICK).unwrap().clone(),
                false => name.clone(),
            };
            let is_default = name == default_audio_node;
            NodeInfo {
                id: (*id).clone().into(),
                name,
                description,
                nickname,
                direction: direction.clone(),
                is_default,
                format: format.clone()
            }
        })
        .collect();
    main_sender.send(MessageResponse::EnumerateNodes(nodes)).unwrap();
}
fn handle_create_stream(
    node_id: GlobalId,
    direction: Direction,
    format: AudioStreamInfo,
    callback: StreamCallback,
    core: Rc<pipewire::core::Core>,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let mut state = state.borrow_mut();
    let node_name = match state.get_node(&node_id) {
        Ok(value) => value.name(),
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let stream_name = match direction {
        Direction::Input => {
            format!("{}.stream_input", node_name)
        }
        Direction::Output => {
            format!("{}.stream_output", node_name)
        }
    };
    let properties = pipewire::properties::properties! {
                    *pipewire::keys::MEDIA_TYPE => MEDIA_TYPE_PROPERTY_VALUE_AUDIO,
                    *pipewire::keys::MEDIA_CLASS => match direction {
                        Direction::Input => MEDIA_CLASS_PROPERTY_VALUE_STREAM_INPUT_AUDIO,
                        Direction::Output => MEDIA_CLASS_PROPERTY_VALUE_STREAM_OUTPUT_AUDIO,
                    },
                };
    let stream = match pipewire::stream::Stream::new(
        &core,
        stream_name.clone().as_str(),
        properties,
    )
        .map_err(move |error| {
            Error {
                description: error.to_string(),
            }
        }) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let mut stream = StreamState::new(
        stream_name.clone(),
        format.into(),
        direction.into(),
        stream
    );
    stream.add_process_listener(
        ListenerTriggerPolicy::Keep,
        callback
    );
    if let Err(value) = state.insert_stream(stream_name.clone(), stream) {
        main_sender
            .send(MessageResponse::Error(value))
            .unwrap();
        return;
    };
    main_sender
        .send(MessageResponse::CreateStream {
            name: stream_name.clone(),
        })
        .unwrap();
}
fn handle_delete_stream(
    name: String,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let mut state = state.borrow_mut();
    let stream = match state.get_stream_mut(&name) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    if stream.is_connected() {
        if let Err(value) = stream.disconnect() {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        };
    }
    if let Err(value) = state.remove_stream(&name) {
        main_sender
            .send(MessageResponse::Error(value))
            .unwrap();
        return;
    };
    main_sender.send(MessageResponse::DeleteStream).unwrap();
}
fn handle_connect_stream(
    name: String,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
)
{
    let mut state = state.borrow_mut();
    let stream = match state.get_stream_mut(&name) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    if let Err(value) = stream.connect() {
        main_sender
            .send(MessageResponse::Error(value))
            .unwrap();
        return;
    };
    main_sender.send(MessageResponse::ConnectStream).unwrap();
}
fn handle_disconnect_stream(
    name: String,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let mut state = state.borrow_mut();
    let stream = match state.get_stream_mut(&name) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    if let Err(value) = stream.disconnect() {
        main_sender
            .send(MessageResponse::Error(value))
            .unwrap();
        return;
    };
    main_sender.send(MessageResponse::DisconnectStream).unwrap();
}
fn handle_check_session_manager_registered(
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    // Checking if session manager is registered because we need "default" metadata
    // object to determine default audio nodes (sink and source).
    fn generate_error_message(session_managers: &Vec<&str>) -> String {
        let session_managers = session_managers.iter()
            .map(move |session_manager| {
                let session_manager = match *session_manager {
                    APPLICATION_NAME_PROPERTY_VALUE_WIRE_PLUMBER => "WirePlumber",
                    APPLICATION_NAME_PROPERTY_VALUE_PIPEWIRE_MEDIA_SESSION => "PipeWire Media Session",
                    _ => panic!("Cannot determine session manager name")
                };
                format!("  - {}", session_manager)
            })
            .collect::<Vec<String>>()
            .join("\n");
        let message = format!(
            "No session manager registered. Install and run one of the following:\n{}",
            session_managers
        );
        message
    }
    let session_managers = vec![
        APPLICATION_NAME_PROPERTY_VALUE_WIRE_PLUMBER,
        APPLICATION_NAME_PROPERTY_VALUE_PIPEWIRE_MEDIA_SESSION
    ];
    let state = state.borrow_mut();
    let clients = state.get_clients().map_err(|_| {
        Error {
            description: generate_error_message(&session_managers),
        }
    });
    let clients = match clients {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let session_manager_registered = clients.iter()
        .any(|(_, client)| {
            session_managers.contains(&client.name.as_str())
        });
    if session_manager_registered {
        return;
    }
    let description = generate_error_message(&session_managers);
    main_sender
        .send(MessageResponse::Error(Error {
            description,
        }))
        .unwrap();
}
fn handle_node_state(
    id: GlobalId,
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) {
    let state = state.borrow();
    let node = match state.get_node(&id) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let state = node.state();
    main_sender.send(MessageResponse::NodeState(state)).unwrap();
}
fn handle_node_states(
    state: Rc<RefCell<GlobalState>>,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
) 
{
    let state = state.borrow_mut();
    let nodes = match state.get_nodes() {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let states = nodes.iter()
        .map(move |(_, node)| {
            node.state()
        })
        .collect::<Vec<_>>();
    main_sender.send(MessageResponse::NodeStates(states)).unwrap();
}
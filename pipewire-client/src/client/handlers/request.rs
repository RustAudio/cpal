use crate::client::channel::{Request, ServerChannel};
use crate::constants::*;
use crate::error::Error;
use crate::listeners::PipewireCoreSync;
use crate::messages::{MessageRequest, MessageResponse, StreamCallback};
use crate::states::{GlobalId, GlobalObjectState, GlobalState, NodeState, OrphanState, StreamState};
use crate::{AudioStreamInfo, Direction, NodeInfo};
use pipewire::proxy::ProxyT;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(test)]
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use pipewire_common::utils::dict_ref_to_hashmap;

struct Context {
    request: Request<MessageRequest>,
    core: Rc<pipewire::core::Core>,
    core_sync: Rc<PipewireCoreSync>,
    main_loop: pipewire::main_loop::MainLoop,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
}

pub(super) fn request_handler(
    core: Rc<pipewire::core::Core>,
    core_sync: Rc<PipewireCoreSync>,
    main_loop: pipewire::main_loop::MainLoop,
    state: Arc<Mutex<GlobalState>>,
    server_channel: ServerChannel<MessageRequest, MessageResponse>,
) -> impl Fn(Request<MessageRequest>) + 'static
{
    move |request| {
        let message_request = request.message.clone();
        let context = Context {
            request,
            core: core.clone(),
            core_sync: core_sync.clone(),
            main_loop: main_loop.clone(),
            state: state.clone(),
            server_channel: server_channel.clone(),
        };
        match message_request {
            MessageRequest::Quit => main_loop.quit(),
            MessageRequest::Settings => handle_settings(
                context,
            ),
            MessageRequest::DefaultAudioNodes => handle_default_audio_nodes(
                context,
            ),
            MessageRequest::GetNode {
                name,
                direction
            } => handle_get_node(context, name, direction),
            MessageRequest::CreateNode {
                name,
                description,
                nickname,
                direction,
                channels,
            } => handle_create_node(
                context,
                name,
                description,
                nickname,
                direction,
                channels,
            ),
            MessageRequest::DeleteNode(id) => handle_delete_node(context, id),
            MessageRequest::EnumerateNodes(direction) => handle_enumerate_node(
                context,
                direction,
            ),
            MessageRequest::CreateStream {
                node_id,
                direction,
                format,
                callback
            } => handle_create_stream(
                context,
                node_id,
                direction,
                format,
                callback,
            ),
            MessageRequest::DeleteStream(name) => handle_delete_stream(
                context,
                name,
            ),
            MessageRequest::ConnectStream(name) => handle_connect_stream(
                context,
                name,
            ),
            MessageRequest::DisconnectStream(name) => handle_disconnect_stream(
                context,
                name,
            ),
            // Internal requests
            MessageRequest::CheckSessionManagerRegistered => handle_check_session_manager_registered(
                context,
            ),
            MessageRequest::SettingsState => handle_settings_state(
                context,
            ),
            MessageRequest::DefaultAudioNodesState => handle_default_audio_nodes_state(
                context,
            ),
            MessageRequest::NodeState(id) => handle_node_state(
                context,
                id,
            ),
            MessageRequest::NodeStates => handle_node_states(
                context,
            ),
            MessageRequest::NodeCount => handle_node_count(
                context,
            ),
            #[cfg(test)]
            MessageRequest::Listeners => handle_listeners(
                context,
            ),
        }
    }
}

fn handle_settings(
    context: Context,
) 
{
    let state = context.state.lock().unwrap();
    let settings = state.get_settings();
    context.server_channel
        .send(&context.request, MessageResponse::Settings(settings))
        .unwrap();
}
fn handle_default_audio_nodes(
    context: Context,
) 
{
    let state = context.state.lock().unwrap();
    let default_audio_devices = state.get_default_audio_nodes();
    context.server_channel
        .send(&context.request, MessageResponse::DefaultAudioNodes(default_audio_devices))
        .unwrap();
}
fn handle_get_node(
    context: Context,
    name: String,
    direction: Direction,
)
{
    let control_flow = RefCell::new(false);
    let state = context.state.lock().unwrap();
    let default_audio_nodes = state.get_default_audio_nodes();
    let default_audio_node = match direction {
        Direction::Input => default_audio_nodes.source.clone(),
        Direction::Output => default_audio_nodes.sink.clone()
    };
    let nodes = match state.get_nodes() {
        Ok(value) => value,
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let node = nodes.iter()
        .find_map(|(id, node)| {
            let properties = node.properties().unwrap();
            let format = node.format().unwrap();
            let name_to_compare = match node.name() {
                Ok(value) => value,
                Err(value) => {
                    control_flow.replace(true);
                    context.server_channel
                        .send(&context.request, MessageResponse::Error(value))
                        .unwrap();
                    return None;
                }
            };
            let direction_to_compare = match node.direction() {
                Ok(value) => value,
                Err(value) => {
                    control_flow.replace(true);
                    context.server_channel
                        .send(&context.request, MessageResponse::Error(value))
                        .unwrap();
                    return None;
                }
            };
            if name_to_compare == name && direction_to_compare == direction {
                Some((id, properties, format))
            } else {
                None
            }
        })
        .iter()
        .find_map(|(id, properties, format)| {
            if *control_flow.borrow() == true {
                return None;
            }
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
            Some(NodeInfo {
                id: (**id).clone().into(),
                name,
                description,
                nickname,
                direction: direction.clone(),
                is_default,
                format: format.clone()
            })
        });
    match node {
        Some(value) => context.server_channel
            .send(&context.request, MessageResponse::GetNode(value))
            .unwrap(),
        None => context.server_channel
            .send(&context.request, MessageResponse::Error(Error {
                description: format!("Node with name({}) not found", name),
            }))
            .unwrap()
    }

}
fn handle_create_node(
    context: Context,
    name: String,
    description: String,
    nickname: String,
    direction: Direction,
    channels: u16,
) 
{
    {
        let control_flow = RefCell::new(false);
        let state = context.state.lock().unwrap();
        let nodes = match state.get_nodes() {
            Ok(value) => value,
            Err(value) => {
                context.server_channel
                    .send(&context.request, MessageResponse::Error(value))
                    .unwrap();
                return;
            }
        };
        let is_exists = nodes.iter().any(|(_, node)| {
            if *control_flow.borrow() == true {
                return false;
            }
            let name_to_compare = match node.name() {
                Ok(value) => value,
                Err(value) => {
                    control_flow.replace(true);
                    context.server_channel
                        .send(&context.request, MessageResponse::Error(value))
                        .unwrap();
                    return false;
                }
            };
            let direction_to_compare = match node.direction() {
                Ok(value) => value,
                Err(value) => {
                    control_flow.replace(true);
                    context.server_channel
                        .send(&context.request, MessageResponse::Error(value))
                        .unwrap();
                    return false;
                }
            };
            name_to_compare == name && direction_to_compare == direction
        });
        if is_exists {
            context.server_channel
                .send(
                    &context.request, 
                    MessageResponse::Error(Error {
                        description: format!("Node with name({}) already exists", name).to_string(),
                    }
                ))
                .unwrap();
        }
    }
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
    let node: pipewire::node::Node = match context.core
        .create_object("adapter", properties)
        .map_err(move |error| {
            Error {
                description: error.to_string(),
            }
        }) {
        Ok(value) => value,
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let core_sync = context.core_sync.clone();
    let listener_server_channel = context.server_channel.clone();
    let listener_state = context.state.clone();
    let listener_properties = properties.clone();
    core_sync.register(
        PIPEWIRE_CORE_SYNC_CREATE_DEVICE_SEQ,
        move |control_flow| {
            let mut state = listener_state.lock().unwrap();
            let mut nodes = match state.get_nodes_mut() {
                Ok(value) => value,
                Err(value) => {
                    listener_server_channel
                        .send(&context.request, MessageResponse::Error(value))
                        .unwrap();
                    control_flow.release();
                    return;
                }
            };
            let node = nodes.iter_mut()
                .find(move |(_, node)| {
                    node.state() == GlobalObjectState::Pending
                });
            match node {
                Some((id, node)) => {
                    let properties = dict_ref_to_hashmap(listener_properties.dict());
                    node.set_properties(properties);
                    listener_server_channel
                        .send(
                            &context.request,
                            MessageResponse::CreateNode((*id).clone())
                        )
                        .unwrap();
                }
                None => {
                    listener_server_channel
                        .send(
                            &context.request,
                            MessageResponse::Error(Error {
                                description: "Created node not found".to_string(),
                            })
                        )
                        .unwrap();
                }
            }
            control_flow.release();
        }
    );
    let mut state = context.state.lock().unwrap();
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
fn handle_delete_node(
    context: Context,
    id: GlobalId,
)
{
    match context.state.lock().unwrap().delete_node(&id) {
        Ok(_) => {
            context.server_channel
                .send(&context.request, MessageResponse::DeleteNode)
                .unwrap()
        }
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap()
        }
    };
}

fn handle_enumerate_node(
    context: Context,
    direction: Direction,
) 
{
    let state = context.state.lock().unwrap();
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
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let nodes: Vec<NodeInfo> = nodes
        .iter()
        .filter_map(|(id, node)| {
            let properties = node.properties().unwrap();
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
    context.server_channel.send(&context.request, MessageResponse::EnumerateNodes(nodes)).unwrap();
}
fn handle_create_stream(
    context: Context,
    node_id: GlobalId,
    direction: Direction,
    format: AudioStreamInfo,
    callback: StreamCallback,
) 
{
    let mut state = context.state.lock().unwrap();
    let node_name = match state.get_node(&node_id) {
        Ok(value) => {
            match value.name() {
                Ok(value) => value,
                Err(value) => {
                    context.server_channel
                        .send(&context.request, MessageResponse::Error(value))
                        .unwrap();
                    return;
                }
            }
        },
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
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
        &context.core,
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
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
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
    stream.add_process_listener(callback);
    if let Err(value) = state.insert_stream(stream_name.clone(), stream) {
        context.server_channel
            .send(&context.request, MessageResponse::Error(value))
            .unwrap();
        return;
    };
    context.server_channel
        .send(
            &context.request,
            MessageResponse::CreateStream(stream_name.clone())
        )
        .unwrap();
}
fn handle_delete_stream(
    context: Context,
    name: String,
) 
{
    let mut state = context.state.lock().unwrap();
    let stream = match state.get_stream_mut(&name) {
        Ok(value) => value,
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    if stream.is_connected() {
        if let Err(value) = stream.disconnect() {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        };
    }
    if let Err(value) = state.delete_stream(&name) {
        context.server_channel
            .send(&context.request, MessageResponse::Error(value))
            .unwrap();
        return;
    };
    context.server_channel.send(&context.request, MessageResponse::DeleteStream).unwrap();
}
fn handle_connect_stream(
    context: Context,
    name: String,
)
{
    let mut state = context.state.lock().unwrap();
    let stream = match state.get_stream_mut(&name) {
        Ok(value) => value,
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    if let Err(value) = stream.connect() {
        context.server_channel
            .send(&context.request, MessageResponse::Error(value))
            .unwrap();
        return;
    };
    context.server_channel.send(&context.request, MessageResponse::ConnectStream).unwrap();
}
fn handle_disconnect_stream(
    context: Context,
    name: String,
) 
{
    let mut state = context.state.lock().unwrap();
    let stream = match state.get_stream_mut(&name) {
        Ok(value) => value,
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    if let Err(value) = stream.disconnect() {
        context.server_channel
            .send(&context.request, MessageResponse::Error(value))
            .unwrap();
        return;
    };
    context.server_channel.send(&context.request, MessageResponse::DisconnectStream).unwrap();
}
fn handle_check_session_manager_registered(
    context: Context,
) 
{
    pub(crate) fn generate_error_message(session_managers: &Vec<&str>) -> String {
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
    // Checking if session manager is registered because we need "default" metadata
    // object to determine default audio nodes (sink and source).
    let session_managers = vec![
        APPLICATION_NAME_PROPERTY_VALUE_WIRE_PLUMBER,
        APPLICATION_NAME_PROPERTY_VALUE_PIPEWIRE_MEDIA_SESSION
    ];
    let error_description = generate_error_message(&session_managers);
    let state = context.state.lock().unwrap();
    let clients = state.get_clients().map_err(|_| {
        Error {
            description: error_description.clone(),
        }
    });
    let clients = match clients {
        Ok(value) => value,
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let session_manager_registered = clients.iter()
        .any(|(_, client)| {
            session_managers.contains(&client.name.as_str())
        });
    context.server_channel
        .send(
            &context.request,
            MessageResponse::CheckSessionManagerRegistered {
                session_manager_registered,
                error: match session_manager_registered {
                    true => Some(Error {
                        description: error_description.clone()
                    }),
                    false => None
                },
            }
        )
        .unwrap();
}
fn handle_settings_state(
    context: Context,
)
{
    let state = context.state.lock().unwrap();
    context.server_channel
        .send(&context.request, MessageResponse::SettingsState(state.get_settings().state))
        .unwrap();
}
fn handle_default_audio_nodes_state(
    context: Context,
)
{
    let state = context.state.lock().unwrap();
    context.server_channel
        .send(&context.request, MessageResponse::DefaultAudioNodesState(state.get_default_audio_nodes().state))
        .unwrap();
}
fn handle_node_state(
    context: Context,
    id: GlobalId,
) {
    let state = context.state.lock().unwrap();
    let node = match state.get_node(&id) {
        Ok(value) => value,
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let state = node.state();
    context.server_channel
        .send(&context.request, MessageResponse::NodeState(state))
        .unwrap();
}
fn handle_node_states(
    context: Context,
) 
{
    let state = context.state.lock().unwrap();
    let nodes = match state.get_nodes() {
        Ok(value) => value,
        Err(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::Error(value))
                .unwrap();
            return;
        }
    };
    let states = nodes.iter()
        .map(move |(_, node)| {
            node.state()
        })
        .collect::<Vec<_>>();
    context.server_channel
        .send(&context.request, MessageResponse::NodeStates(states))
        .unwrap();
}
fn handle_node_count(
    context: Context,
)
{
    let state = context.state.lock().unwrap();
    match state.get_nodes() {
        Ok(value) => {
            context.server_channel
                .send(&context.request, MessageResponse::NodeCount(value.len() as u32))
                .unwrap();
        },
        Err(_) => {
            context.server_channel
                .send(&context.request, MessageResponse::NodeCount(0))
                .unwrap();
        }
    };
}
#[cfg(test)]
fn handle_listeners(
    context: Context,
)
{
    let state = context.state.lock().unwrap();
    let mut core = HashMap::new();
    core.insert("0".to_string(), context.core_sync.get_listener_names());
    let metadata = state.get_metadatas()
        .unwrap_or_default()
        .iter()
        .map(move |(id, metadata)| {
            (id.to_string(), metadata.get_listener_names())
        })
        .collect::<HashMap<_, _>>();
    let nodes = state.get_nodes()
        .unwrap_or_default()
        .iter()
        .map(move |(id, node)| {
            (id.to_string(), node.get_listener_names())
        })
        .collect::<HashMap<_, _>>();
    let streams = state.get_streams()
        .unwrap_or_default()
        .iter()
        .map(move |(name, stream)| {
            ((*name).clone(), stream.get_listener_names())
        })
        .collect::<HashMap<_, _>>();
    context.server_channel
        .send(
            &context.request,
            MessageResponse::Listeners {
                core,
                metadata,
                nodes,
                streams,
            }
        )
        .unwrap();
}
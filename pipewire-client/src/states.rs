use super::constants::*;
use crate::error::Error;
use crate::listeners::{Listener, ListenerTriggerPolicy, Listeners};
use crate::messages::StreamCallback;
use crate::utils::dict_ref_to_hashmap;
use crate::Direction;
use pipewire::spa::utils::dict::ParsableValue;
use pipewire_spa_utils::audio::raw::AudioInfoRaw;
use pipewire_spa_utils::audio::AudioChannel;
use pipewire_spa_utils::format::{MediaSubtype, MediaType};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::io::Cursor;
use std::rc::Rc;
use std::str::FromStr;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GlobalId(u32);

impl From<String> for GlobalId {
    fn from(value: String) -> Self {
        u32::parse_value(value.as_str()).unwrap().into()
    }
}

impl From<u32> for GlobalId {
    fn from(value: u32) -> Self {
        GlobalId(value)
    }
}

impl From<i32> for GlobalId {
    fn from(value: i32) -> Self {
        GlobalId(value as u32)
    }
}

impl Into<i32> for GlobalId {
    fn into(self) -> i32 {
        self.0 as i32
    }
}

impl From<GlobalId> for u32 {
    fn from(value: GlobalId) -> Self {
        value.0
    }
}

impl Display for GlobalId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum GlobalObjectState {
    Pending,
    Initialized
}

pub(super) struct GlobalState {
    orphans: Rc<RefCell<HashMap<usize, OrphanState>>>,
    clients: HashMap<GlobalId, ClientState>,
    metadata: HashMap<GlobalId, MetadataState>,
    nodes: HashMap<GlobalId, NodeState>,
    streams: HashMap<String, StreamState>,
    settings: SettingsState,
    default_audio_nodes: DefaultAudioNodesState,
}

impl GlobalState {
    pub fn insert_orphan(&mut self, mut state: OrphanState) {
        let index = std::ptr::addr_of!(state) as usize;
        let listener_orphans = self.orphans.clone();
        state.add_removed_listener(
            ListenerTriggerPolicy::Remove,
            move || {
                listener_orphans.borrow_mut().remove(&index);
            }
        );
        self.orphans.borrow_mut().insert(index, state);
    }

    pub fn insert_client(&mut self, id: GlobalId, state: ClientState) -> Result<(), Error> {
        if self.clients.contains_key(&id) {
            return Err(Error {
                description: format!("Client with id({}) already exists", id),
            });
        }
        self.clients.insert(id, state);
        Ok(())
    }

    pub fn get_clients(&self) -> Result<HashMap<&GlobalId ,&ClientState>, Error> {
        let clients = self.clients.iter()
            .map(|(id, state)| (id, state))
            .collect::<HashMap<_, _>>();
        if clients.is_empty() {
            return Err(Error {
                description: "Zero client registered".to_string(),
            })
        }
        Ok(clients)
    }

    pub fn insert_metadata(&mut self, id: GlobalId, state: MetadataState) -> Result<(), Error> {
        if self.metadata.contains_key(&id) {
            return Err(Error {
                description: format!("Metadata with id({}) already exists", id),
            });
        }
        self.metadata.insert(id, state);
        Ok(())
    }

    pub fn get_metadata(&self, id: &GlobalId) -> Result<&MetadataState, Error> {
        self.metadata.get(id).ok_or(Error {
            description: format!("Metadata with id({}) not found", id),
        })
    }

    pub fn get_metadata_mut(&mut self, id: &GlobalId) -> Result<&mut MetadataState, Error> {
        self.metadata.get_mut(id).ok_or(Error {
            description: format!("Metadata with id({}) not found", id),
        })
    }

    pub fn insert_node(&mut self, id: GlobalId, state: NodeState) -> Result<(), Error> {
        if self.nodes.contains_key(&id) {
            return Err(Error {
                description: format!("Node with id({}) already exists", id),
            });
        }
        self.nodes.insert(id, state);
        Ok(())
    }

    pub fn get_node(&self, id: &GlobalId) -> Result<&NodeState, Error> {
        self.nodes.get(id).ok_or(Error {
            description: format!("Node with id({}) not found", id),
        })
    }

    pub fn get_node_mut(&mut self, id: &GlobalId) -> Result<&mut NodeState, Error> {
        self.nodes.get_mut(id).ok_or(Error {
            description: format!("Node with id({}) not found", id),
        })
    }

    pub fn get_nodes(&self) -> Result<HashMap<&GlobalId ,&NodeState>, Error> {
        let nodes = self.nodes.iter()
            .map(|(id, state)| (id, state))
            .collect::<HashMap<_, _>>();
        if nodes.is_empty() {
            return Err(Error {
                description: "Zero node registered".to_string(),
            })
        }
        Ok(nodes)
    }

    pub fn insert_stream(&mut self, name: String, state: StreamState) -> Result<(), Error> {
        if self.streams.contains_key(&name) {
            return Err(Error {
                description: format!("Stream with name({}) already exists", name),
            });
        }
        self.streams.insert(name, state);
        Ok(())
    }

    pub fn remove_stream(&mut self, name: &String) -> Result<(), Error> {
        if self.streams.contains_key(name) == false {
            return Err(Error {
                description: format!("Stream with name({}) not found", name),
            });
        }
        self.streams.remove(name);
        Ok(())
    }

    pub fn get_stream(&self, name: &String) -> Result<&StreamState, Error> {
        self.streams.get(name).ok_or(Error {
            description: format!("Stream with name({}) not found", name),
        })
    }

    pub fn get_stream_mut(&mut self, name: &String) -> Result<&mut StreamState, Error> {
        self.streams.get_mut(name).ok_or(Error {
            description: format!("Stream with name({}) not found", name),
        })
    }

    pub fn get_settings(&self) -> SettingsState {
        self.settings.clone()
    }

    pub fn get_default_audio_nodes(&self) -> DefaultAudioNodesState {
        self.default_audio_nodes.clone()
    }

    pub fn remove(&mut self, id: &GlobalId) {
        self.metadata.remove(id);
        self.nodes.remove(id);
    }
}

impl Default for GlobalState {
    fn default() -> Self {
        GlobalState {
            orphans: Rc::new(RefCell::new(HashMap::new())),
            clients: HashMap::new(),
            metadata: HashMap::new(),
            nodes: HashMap::new(),
            streams: HashMap::new(),
            settings: SettingsState::default(),
            default_audio_nodes: DefaultAudioNodesState::default(),
        }
    }
}

pub(super) struct OrphanState {
    proxy: pipewire::proxy::Proxy,
    listeners: Rc<RefCell<Listeners<pipewire::proxy::ProxyListener>>>
}

impl OrphanState {
    pub fn new(proxy: pipewire::proxy::Proxy) -> Self {
        Self {
            proxy,
            listeners: Rc::new(RefCell::new(Listeners::new())),
        }
    }

    pub fn add_removed_listener<F>(&mut self, policy: ListenerTriggerPolicy, callback: F)
    where
        F: Fn() + 'static
    {
        const LISTENER_NAME: &str = "removed";
        let listeners = self.listeners.clone();
        let listener = self.proxy.add_listener_local()
            .removed(move || {
                callback();
                listeners.borrow_mut().triggered(&LISTENER_NAME.to_string());
            })
            .register();
        self.listeners.borrow_mut().add(
            LISTENER_NAME.to_string(),
            Listener::new(listener, policy)
        );
    }
}

pub(super) struct NodeState {
    proxy: pipewire::node::Node,
    state: Rc<RefCell<GlobalObjectState>>,
    properties: Rc<RefCell<HashMap<String, String>>>,
    format: Rc<RefCell<Option<AudioInfoRaw>>>,
    listeners: Rc<RefCell<Listeners<pipewire::node::NodeListener>>>
}

impl NodeState {
    pub fn new(proxy: pipewire::node::Node) -> Self {
        Self {
            proxy,
            state: Rc::new(RefCell::new(GlobalObjectState::Pending)),
            properties: Rc::new(RefCell::new(HashMap::new())),
            format: Rc::new(RefCell::new(None)),
            listeners: Rc::new(RefCell::new(Listeners::new())),
        }
    }
    
    pub fn state(&self) -> GlobalObjectState {
        self.state.borrow().clone()
    }

    fn set_state(&mut self) {
        let properties = self.properties.borrow();
        let format = self.format.borrow();
        
        let new_state = if properties.is_empty() == false && format.is_some() {
            GlobalObjectState::Initialized
        } else {
            GlobalObjectState::Pending
        };
        
        let mut state = self.state.borrow_mut();
        *state = new_state;
    }
    
    pub fn properties(&self) -> HashMap<String, String> {
        self.properties.borrow().clone()
    }
    
    pub fn set_properties(&mut self, properties: HashMap<String, String>) {
        self.properties.borrow_mut().extend(properties);
        self.set_state();
    }
    
    pub fn format(&self) -> Option<AudioInfoRaw> {
        self.format.borrow().clone()
    }
    
    pub fn set_format(&mut self, format: AudioInfoRaw) {
        *self.format.borrow_mut() = Some(format);
        self.set_state();
    }
    
    pub fn name(&self) -> String {
        self.properties.borrow().get(*pipewire::keys::NODE_NAME).unwrap().clone()
    }

    fn add_info_listener<F>(&mut self, name: String, policy: ListenerTriggerPolicy, listener: F)
    where
        F: Fn(&pipewire::node::NodeInfoRef) + 'static
    {
        let listeners = self.listeners.clone();
        let listener_name = name.clone();
        let listener = self.proxy.add_listener_local()
            .info(move |info| {
                listener(info);
                listeners.borrow_mut().triggered(&listener_name);
            })
            .register();
        self.listeners.borrow_mut().add(name, Listener::new(listener, policy));
    }

    pub fn add_properties_listener<F>(&mut self, policy: ListenerTriggerPolicy, callback: F)
    where
        F: Fn(HashMap<String, String>) + 'static,
    {
        self.add_info_listener(
            "properties".to_string(),
            policy,
            move |info| {
                if info.props().is_none() {
                    return;
                }
                let properties = info.props().unwrap();
                let properties = dict_ref_to_hashmap(properties);
                callback(properties);
            }
        );
    }

    fn add_parameter_listener<F>(
        &mut self,
        name: String,
        expected_kind: pipewire::spa::param::ParamType,
        policy: ListenerTriggerPolicy,
        listener: F
    )
    where
        F: Fn(u32, u32, &pipewire::spa::pod::Pod) + 'static,
    {
        let listeners = self.listeners.clone();
        let listener_name = name.clone();
        self.proxy.subscribe_params(&[expected_kind]);
        let listener = self.proxy.add_listener_local()
            .param(move |_, kind, id, next_id, parameter| {
                if kind != expected_kind {
                    return;
                }
                let Some(parameter) = parameter else {
                    return;
                };
                listener(id, next_id, parameter);
                listeners.borrow_mut().triggered(&listener_name);
            })
            .register();
        self.listeners.borrow_mut().add(name, Listener::new(listener, policy));
    }

    pub fn add_format_listener<F>(&mut self, policy: ListenerTriggerPolicy, callback: F)
    where
        F: Fn(Result<AudioInfoRaw, Error>) + 'static,
    {
        self.add_parameter_listener(
            "format".to_string(),
            pipewire::spa::param::ParamType::EnumFormat,
            policy,
            move |_, _, parameter| {
                let (media_type, media_subtype): (MediaType, MediaSubtype) =
                    match pipewire::spa::param::format_utils::parse_format(parameter) {
                        Ok((media_type, media_subtype)) => (media_type.0.into(), media_subtype.0.into()),
                        Err(_) => return,
                    };
                let pod = parameter;
                let data = pod.as_bytes();
                let parameter = match media_type {
                    MediaType::Audio => match media_subtype {
                        MediaSubtype::Raw => {
                            let result = pipewire::spa::pod::deserialize::PodDeserializer::deserialize_from(data);
                            let result = result
                                .map(move |(_, parameter)| {
                                    parameter
                                })
                                .map_err(move |error| {
                                    let description = match error {
                                        pipewire::spa::pod::deserialize::DeserializeError::Nom(_) => "Parsing error",
                                        pipewire::spa::pod::deserialize::DeserializeError::UnsupportedType => "Unsupported type",
                                        pipewire::spa::pod::deserialize::DeserializeError::InvalidType => "Invalid type",
                                        pipewire::spa::pod::deserialize::DeserializeError::PropertyMissing => "Property missing",
                                        pipewire::spa::pod::deserialize::DeserializeError::PropertyWrongKey(value) => &*format!(
                                            "Wrong property key({})", 
                                            value
                                        ),
                                        pipewire::spa::pod::deserialize::DeserializeError::InvalidChoiceType => "Invalide choice type",
                                        pipewire::spa::pod::deserialize::DeserializeError::MissingChoiceValues => "Missing choice values",
                                    };
                                    Error {
                                        description: format!(
                                            "Failed POD deserialization for type(AudioInfoRaw): {}", 
                                            description
                                        ),
                                    }
                                });
                            result
                        }
                        _ => return
                    },
                    _ => return
                };
                callback(parameter);
            }
        );
    }
}

pub(super) struct ClientState {
    pub(super) name: String
}

impl ClientState {
    pub fn new(name: String) -> Self {
        Self {
            name,
        }
    }
}

pub(super) struct MetadataState {
    proxy: pipewire::metadata::Metadata,
    pub(super) state: Rc<RefCell<GlobalObjectState>>,
    pub(super) name: String,
    listeners: Rc<RefCell<Listeners<pipewire::metadata::MetadataListener>>>,
}

impl MetadataState {
    pub fn new(proxy: pipewire::metadata::Metadata, name: String) -> Self {
        Self {
            proxy,
            name,
            state: Rc::new(RefCell::new(GlobalObjectState::Pending)),
            listeners: Rc::new(RefCell::new(Listeners::new())),
        }
    }

    pub fn add_property_listener<F>(&mut self, policy: ListenerTriggerPolicy, listener: F)
    where
        F: Fn(u32, Option<&str>, Option<&str>, Option<&str>) -> i32 + Sized + 'static
    {
        const LISTENER_NAME: &str = "property";
        let listeners = self.listeners.clone();
        let listener = self.proxy.add_listener_local()
            .property(move |subject , key, kind, value| {
                let result = listener(subject, key, kind, value);
                listeners.borrow_mut().triggered(&LISTENER_NAME.to_string());
                result
            })
            .register();
        self.listeners.borrow_mut().add(
            LISTENER_NAME.to_string(),
            Listener::new(listener, policy)
        );
    }
}

#[derive(Debug, Default)]
pub(super) struct StreamUserData {}

pub(super) struct StreamState {
    proxy: pipewire::stream::Stream,
    pub(super) name: String,
    is_connected: bool,
    format: pipewire::spa::param::audio::AudioInfoRaw,
    direction: pipewire::spa::utils::Direction,
    listeners: Rc<RefCell<Listeners<pipewire::stream::StreamListener<StreamUserData>>>>,
}

impl StreamState {
    pub fn new(
        name: String,
        format: pipewire::spa::param::audio::AudioInfoRaw,
        direction: pipewire::spa::utils::Direction,
        proxy: pipewire::stream::Stream
    ) -> Self {
        Self {
            name,
            proxy,
            is_connected: false,
            format,
            direction,
            listeners: Rc::new(RefCell::new(Listeners::new())),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected
    }

    pub fn connect(&mut self) -> Result<(), Error> {
        if self.is_connected {
            return Err(Error {
                description: format!("Stream {} is already connected", self.name)
            });
        }
        
        let object = pipewire::spa::pod::Value::Object(pipewire::spa::pod::Object {
            type_: pipewire::spa::sys::SPA_TYPE_OBJECT_Format,
            id: pipewire::spa::sys::SPA_PARAM_EnumFormat,
            properties: self.format.into(),
        });

        let values: Vec<u8> = pipewire::spa::pod::serialize::PodSerializer::serialize(
            Cursor::new(Vec::new()),
            &object,
        )
        .unwrap()
        .0
        .into_inner();

        let mut params = [pipewire::spa::pod::Pod::from_bytes(&values).unwrap()];
        let flags = pipewire::stream::StreamFlags::AUTOCONNECT | pipewire::stream::StreamFlags::MAP_BUFFERS;

        self.proxy
            .connect(
                self.direction,
                None,
                flags,
                &mut params,
            )
            .map_err(move |error| Error { description: error.to_string() })?;

        self.is_connected = true;

        Ok(())
    }
    
    pub fn disconnect(&mut self) -> Result<(), Error> {
        if self.is_connected == false {
            return Err(Error {
                description: format!("Stream {} is not connected", self.name)
            });
        }
        self.proxy
            .disconnect()
            .map_err(move |error| Error { description: error.to_string() })?;

        self.is_connected = false;

        Ok(())
    }

    pub fn add_process_listener(
        &mut self,
        policy: ListenerTriggerPolicy,
        mut callback: StreamCallback
    )
    {
        const LISTENER_NAME: &str = "process";
        let listeners = self.listeners.clone();
        let listener = self.proxy.add_local_listener()
            .process(move |stream, _| {
                let buffer = stream.dequeue_buffer().unwrap();
                callback.call(buffer);
                listeners.borrow_mut().triggered(&LISTENER_NAME.to_string());
            })
            .register()
            .unwrap();
        self.listeners.borrow_mut().add(LISTENER_NAME.to_string(), Listener::new(listener, policy));
    }
}

pub(super) struct PortStateProperties {
    path: String,
    channel: AudioChannel,
    id: GlobalId,
    name: String,
    direction: Direction,
    alias: String,
    group: String,
}

impl From<&pipewire::spa::utils::dict::DictRef> for PortStateProperties {
    fn from(value: &pipewire::spa::utils::dict::DictRef) -> Self {
        let properties = dict_ref_to_hashmap(value);
        let path = properties.get("object.path").unwrap().to_string();
        let channel = properties.get("audio.channel").unwrap().to_string();
        let id = properties.get("port.id").unwrap().to_string();
        let name = properties.get("port.name").unwrap().to_string();
        let direction = properties.get("port.direction").unwrap().to_string();
        let alias = properties.get("port.alias").unwrap().to_string();
        let group = properties.get("port.group").unwrap().to_string();
        Self {
            path,
            channel: AudioChannel::UNKNOWN,
            id: id.into(),
            name,
            direction: match direction.as_str() {
                "in" => Direction::Input,
                "out" => Direction::Output,
                &_ => panic!("Cannot determine direction: {}", direction.as_str()),
            },
            alias,
            group,
        }
    }
}

pub(super) struct PortState {
    proxy: pipewire::link::Link,
    properties: Rc<RefCell<Option<PortStateProperties>>>,
    pub(super) state: Rc<RefCell<GlobalObjectState>>,
    listeners: Rc<RefCell<Listeners<pipewire::stream::StreamListener<StreamUserData>>>>,
}

// impl PortState {
//     pub fn new(proxy: pipewire::port::Port) {
//         proxy.add_listener_local().info(move |x| {
//             x.
//         })
//             .param(move |subject , key, kind, value| {
// 
//             })
//     }
// }

pub(super) struct LinkState {
    proxy: pipewire::link::Link,
    input_node_id: GlobalId,
    input_port_id: GlobalId,
    output_node_id: GlobalId,
    output_port_id: GlobalId,
    pub(super) state: Rc<RefCell<GlobalObjectState>>,
    listeners: Rc<RefCell<Listeners<pipewire::stream::StreamListener<StreamUserData>>>>,
}

#[derive(Debug, Clone)]
pub struct SettingsState {
    pub(super) state: GlobalObjectState,
    pub allowed_sample_rates: Vec<u32>,
    pub sample_rate: u32,
    pub min_buffer_size: u32,
    pub max_buffer_size: u32,
    pub default_buffer_size: u32,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            state: GlobalObjectState::Pending,
            allowed_sample_rates: vec![],
            sample_rate: 0,
            min_buffer_size: 0,
            max_buffer_size: 0,
            default_buffer_size: 0,
        }
    }
}

impl SettingsState {
    pub(super) fn listener(state: Rc<RefCell<GlobalState>>) -> impl Fn(u32, Option<&str>, Option<&str>, Option<&str>) -> i32 + 'static
    {
        const EXPECTED_PROPERTY: u32 = 5;
        let property_count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        move |_: u32, key: Option<&str>, _: Option<&str>, value: Option<&str>| {
            let settings = &mut state.borrow_mut().settings;
            let key = key.unwrap();
            let value = value.unwrap();
            match key {
                CLOCK_RATE_PROPERTY_KEY => {
                    settings.sample_rate = u32::from_str(value).unwrap();
                    property_count.set(property_count.get() + 1);
                },
                CLOCK_QUANTUM_PROPERTY_KEY => {
                    settings.default_buffer_size = u32::from_str(value).unwrap();
                    property_count.set(property_count.get() + 1);
                }
                CLOCK_QUANTUM_MIN_PROPERTY_KEY => {
                    settings.min_buffer_size = u32::from_str(value).unwrap();
                    property_count.set(property_count.get() + 1);
                }
                CLOCK_QUANTUM_MAX_PROPERTY_KEY => {
                    settings.max_buffer_size = u32::from_str(value).unwrap();
                    property_count.set(property_count.get() + 1);
                }
                CLOCK_ALLOWED_RATES_PROPERTY_KEY => {
                    let rates: Result<Vec<u32>, _> = value[2..value.len() - 2]
                        .split_whitespace()
                        .map(|x| x.parse::<u32>())
                        .collect();
                    settings.allowed_sample_rates = rates.unwrap();
                    property_count.set(property_count.get() + 1);
                }
                &_ => {}
            };
            if let (GlobalObjectState::Pending, EXPECTED_PROPERTY) = (settings.state.clone(), property_count.get()) {
                settings.state = GlobalObjectState::Initialized;
            }
            0
        }
    }
}

#[derive(Debug, Clone)]
pub struct DefaultAudioNodesState {
    pub(super) state: GlobalObjectState,
    pub source: String,
    pub sink: String,
}

impl Default for DefaultAudioNodesState {
    fn default() -> Self {
        Self {
            state: GlobalObjectState::Pending,
            source: "".to_string(),
            sink: "".to_string(),
        }
    }
}

impl DefaultAudioNodesState {
    pub(super) fn listener(state: Rc<RefCell<GlobalState>>) -> impl Fn(u32, Option<&str>, Option<&str>, Option<&str>) -> i32 + 'static
    {
        const EXPECTED_PROPERTY: u32 = 2;
        let property_count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        move |_: u32, key: Option<&str>, _: Option<&str>, value: Option<&str>| {
            let default_audio_devices = &mut state.borrow_mut().default_audio_nodes;
            let key = key.unwrap();
            if value.is_none() {
                return 0;
            }
            let value = value.unwrap();
            match key {
                DEFAULT_AUDIO_SINK_PROPERTY_KEY => {
                    let value: serde_json::Value = serde_json::from_str(value).unwrap();
                    default_audio_devices.sink = value.as_object()
                        .unwrap()
                        .get("name")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();
                    property_count.set(property_count.get() + 1);
                },
                DEFAULT_AUDIO_SOURCE_PROPERTY_KEY => {
                    let value: serde_json::Value = serde_json::from_str(value).unwrap();
                    default_audio_devices.source = value.as_object()
                        .unwrap()
                        .get("name")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();
                    property_count.set(property_count.get() + 1);
                },
                &_ => {}
            };
            if let (GlobalObjectState::Pending, EXPECTED_PROPERTY) = (default_audio_devices.state.clone(), property_count.get()) {
                default_audio_devices.state = GlobalObjectState::Initialized;
            }
            0
        }
    }
}
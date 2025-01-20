use crate::error::Error;
use crate::info::{AudioStreamInfo, NodeInfo};
use crate::states::{DefaultAudioNodesState, GlobalId, GlobalObjectState, SettingsState};
use crate::utils::Direction;
use pipewire_spa_utils::audio::raw::AudioInfoRaw;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Mutex};
use crate::listeners::ListenerControlFlow;

pub(super) struct StreamCallback {
    callback: Arc<Mutex<Box<dyn FnMut(&mut ListenerControlFlow, pipewire::buffer::Buffer) + Send + 'static>>>
}

impl <F: FnMut(&mut ListenerControlFlow, pipewire::buffer::Buffer) + Send + 'static> From<F> for StreamCallback {
    fn from(value: F) -> Self {
        Self { callback: Arc::new(Mutex::new(Box::new(value))) }
    }
}

impl StreamCallback {
    pub fn call(&mut self, control_flow: &mut ListenerControlFlow, buffer: pipewire::buffer::Buffer) {
        let mut callback = self.callback.lock().unwrap();
        callback(control_flow, buffer);
    }
}

impl Debug for StreamCallback {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamCallback").finish()
    }
}

impl Clone for StreamCallback {
    fn clone(&self) -> Self {
        Self { callback: self.callback.clone() }
    }
}

#[derive(Debug, Clone)]
pub(super) enum MessageRequest {
    Quit,
    Settings,
    DefaultAudioNodes,
    CreateNode {
        name: String,
        description: String,
        nickname: String,
        direction: Direction,
        channels: u16,
    },
    EnumerateNodes(Direction),
    CreateStream {
        node_id: GlobalId,
        direction: Direction,
        format: AudioStreamInfo,
        callback: StreamCallback,
    },
    DeleteStream {
        name: String
    },
    ConnectStream {
        name: String
    },
    DisconnectStream {
        name: String
    },
    // Internal requests
    CheckSessionManagerRegistered,
    SettingsState,
    DefaultAudioNodesState,
    NodeState(GlobalId),
    NodeStates,
    NodeCount,
    Listeners
}

#[derive(Debug, Clone)]
pub(super) enum MessageResponse {
    Error(Error),
    Initialized,
    Settings(SettingsState),
    DefaultAudioNodes(DefaultAudioNodesState),
    CreateNode {
        id: GlobalId
    },
    EnumerateNodes(Vec<NodeInfo>),
    CreateStream {
        name: String,
    },
    DeleteStream,
    ConnectStream,
    DisconnectStream,
    // Internals responses
    CheckSessionManagerRegistered {
        session_manager_registered: bool,
        error: Option<Error>,
    },
    SettingsState(GlobalObjectState),
    DefaultAudioNodesState(GlobalObjectState),
    NodeState(GlobalObjectState),
    NodeStates(Vec<GlobalObjectState>),
    NodeCount(u32),
    // For testing purpose only
    Listeners {
        core: HashMap<String, Vec<String>>,
        metadata: HashMap<String, Vec<String>>,
        nodes: HashMap<String, Vec<String>>,
        streams: HashMap<String, Vec<String>>,
    }
}

#[derive(Debug, Clone)]
pub(super) enum EventMessage {
    SetMetadataListeners {
        id: GlobalId
    },
    RemoveNode {
        id: GlobalId
    },
    SetNodePropertiesListener {
        id: GlobalId
    },
    SetNodeFormatListener{
        id: GlobalId
    },
    SetNodeProperties {
        id: GlobalId,
        properties: HashMap<String, String>,
    },
    SetNodeFormat {
        id: GlobalId,
        format: AudioInfoRaw,
    },
}
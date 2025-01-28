use std::any::TypeId;
use std::collections::hash_map::Iter;
use std::collections::HashMap;
use crate::{NodeInfo, PipewireClient};
use pipewire_common::utils::Direction;
use pipewire_test_utils::server::{server_with_default_configuration, server_without_node, server_without_session_manager, Server};
use rstest::{fixture, Context};
use std::fmt::{Display, Formatter};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::{mem, thread};
use std::ptr::drop_in_place;
use std::time::Duration;
use ctor::{ctor, dtor};
use libc::{atexit, signal, SIGINT, SIGSEGV, SIGTERM};
use tokio::runtime::Runtime;
use uuid::Uuid;
use pipewire_common::error::Error;
use pipewire_test_utils::environment::{SHARED_SERVER, TEST_ENVIRONMENT};
use crate::states::StreamState;

pub struct NodeInfoFixture {
    client: Arc<PipewireClient>,
    node: OnceLock<NodeInfo>,
    direction: Direction
}

impl NodeInfoFixture {
    pub(self) fn new(client: Arc<PipewireClient>, direction: Direction) -> Self {
        Self {
            client,
            node: OnceLock::new(),
            direction,
        }
    }

    pub fn client(&self) -> Arc<PipewireClient> {
        self.client.clone()
    }
}

impl Deref for NodeInfoFixture {
    type Target = NodeInfo;

    fn deref(&self) -> &Self::Target {
        let node = self.node.get_or_init(|| {
            let node_name = Uuid::new_v4().to_string();
            self.client.node()
                .create(
                    node_name.clone(),
                    node_name.clone(),
                    node_name.clone(),
                    self.direction.clone(),
                    2
                ).unwrap();
            let node = self.client.node().get(node_name, self.direction.clone()).unwrap();
            node
        });
        node
    }
}

impl Drop for NodeInfoFixture {
    fn drop(&mut self) {
        self.client.node()
            .delete(self.node.get().unwrap().id)
            .unwrap()
    }
}

pub struct StreamFixture {
    client: Arc<PipewireClient>,
    node: NodeInfoFixture,
    stream: OnceLock<String>,
    direction: Direction
}

impl StreamFixture {
    pub(self) fn new(client: Arc<PipewireClient>, node: NodeInfoFixture) -> Self {
        let direction = node.direction.clone();
        Self {
            client: client.clone(),
            node,
            stream: OnceLock::new(),
            direction,
        }
    }

    pub fn client(&self) -> Arc<PipewireClient> {
        self.client.clone()
    }

    pub fn name(&self) -> String {
        self.deref().clone()
    }

    pub fn listeners(&self) -> HashMap<String, Vec<String>> {
        let listeners = self.client.core().get_listeners().unwrap();
        let stream_listeners = listeners.get(&TypeId::of::<StreamState>()).unwrap();
        stream_listeners.clone()
    }

    pub fn connect(&self) -> Result<(), Error> {
        let stream = self.deref().clone();
        self.client.stream().connect(stream)
    }

    pub fn disconnect(&self) -> Result<(), Error> {
        let stream = self.deref().clone();
        self.client.stream().disconnect(stream)
    }

    pub fn delete(&self) -> Result<(), Error> {
        let stream = self.deref().clone();
        self.client.stream().delete(stream)
    }
}

impl Display for StreamFixture {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.deref().clone())
    }
}

impl Deref for StreamFixture {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        let stream = self.stream.get_or_init(|| {
            self.client.stream()
                .create(
                    self.node.id,
                    self.direction.clone(),
                    self.node.format.clone().into(),
                    move |control_flow, _| {
                        assert!(true);
                        control_flow.release();
                    }
                ).unwrap()
        });
        stream
    }
}

impl Drop for StreamFixture {
    fn drop(&mut self) {
        let stream = self.stream.get().unwrap().clone();
        let result = self.client.stream().delete(stream.clone());
        match result {
            Ok(_) => {}
            Err(value) => {
                let error_message = format!(
                    "Stream with name({}) not found", 
                    self.stream.get().unwrap().clone()
                );
                if error_message != value.description {
                    panic!("{}", error_message);
                }
                // If error is raised, we can assume this stream had been deleted. 
                // Certainly due to delete tests, we cannot be sure at this point but let just 
                // show a warning for now.
                eprintln!(
                    "Failed to delete stream: {}. Stream delete occurred during test method ?", 
                    self.stream.get().unwrap()
                );
            }
        }
    }
}

pub struct ConnectedStreamFixture {
    client: Arc<PipewireClient>,
    stream: StreamFixture,
}

impl ConnectedStreamFixture {
    pub(self) fn new(client: Arc<PipewireClient>, stream: StreamFixture) -> Self {
        stream.connect().unwrap();
        Self {
            client,
            stream,
        }
    }

    pub fn disconnect(&self) -> Result<(), Error> {
        let stream = self.stream.deref().clone();
        self.client.stream().disconnect(stream)
    }
}

impl Display for ConnectedStreamFixture {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.stream.fmt(f)
    }
}

impl Deref for ConnectedStreamFixture {
    type Target = StreamFixture;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl Drop for ConnectedStreamFixture {
    fn drop(&mut self) {
        let stream = self.stream.deref().clone();
        let result = self.client.stream().disconnect(stream.clone());
        match result {
            Ok(_) => {}
            Err(value) => {
                let error_message = format!(
                    "Stream {} is not connected",
                    stream.clone()
                );
                if error_message != value.description {
                    panic!("{}", error_message);
                }
                // If error is raised, we can assume this stream had been disconnected. 
                // Certainly due to disconnect tests, we cannot be sure at this point but let just 
                // show a warning for now.
                eprintln!(
                    "Failed to disconnect stream: {}. Stream disconnect occurred during test method ?",
                    stream.clone()
                );
            }
        }
    }
}

pub struct PipewireTestClient {
    name: String,
    server: Arc<Server>,
    client: Arc<PipewireClient>,
}

impl PipewireTestClient {
    pub(self) fn new(
        name: String,
        server: Arc<Server>,
        client: PipewireClient
    ) -> Self {
        let client = Arc::new(client);
        println!("Create {} client: {}", name.clone(), Arc::strong_count(&client));
        Self {
            name,
            server,
            client: client.clone(),
        }
    }

    pub(self) fn reference_count(&self) -> usize {
        Arc::strong_count(&self.client)
    }

    pub(self) fn create_input_node(&self) -> NodeInfoFixture {
        NodeInfoFixture::new(self.client.clone(), Direction::Input)
    }

    pub(self) fn create_output_node(&self) -> NodeInfoFixture {
        NodeInfoFixture::new(self.client.clone(), Direction::Output)
    }
    
    pub(self) unsafe fn cleanup(&mut self) {
        let pointer = std::ptr::addr_of_mut!(self.client);
        let reference_count = Arc::strong_count(&self.client);
        for _ in 0..reference_count {
            drop_in_place(pointer);
        }
    }
}

impl Clone for PipewireTestClient {
    fn clone(&self) -> Self {
        let client = Self {
            name: self.name.clone(),
            server: self.server.clone(),
            client: self.client.clone(),
        };
        println!("Clone {} client: {}", self.name.clone(), self.reference_count());
        client
    }
}

impl Deref for PipewireTestClient {
    type Target = Arc<PipewireClient>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl Drop for PipewireTestClient {
    fn drop(&mut self) {
        println!("Drop {} client: {}", self.name.clone(), self.reference_count() - 1);
    }
}

#[ctor]
static SHARED_CLIENT: Arc<Mutex<PipewireTestClient>> = {
    unsafe { libc::printf("Initialize shared client\n\0".as_ptr() as *const i8); };
    let server = SHARED_SERVER.clone();
    let client = PipewireTestClient::new(
        "shared".to_string(),
        server,
        PipewireClient::new(
            Arc::new(Runtime::new().unwrap()),
            TEST_ENVIRONMENT.lock().unwrap().client_timeout.clone(),
        ).unwrap(),
    );
    Arc::new(Mutex::new(client))
};

#[dtor]
unsafe fn cleanup_shared_client() {
    libc::printf("Cleaning shared client\n\0".as_ptr() as *const i8);
    SHARED_CLIENT.lock().unwrap().cleanup();
}

#[fixture]
pub fn isolated_client() -> PipewireTestClient {
    let server = SHARED_SERVER.clone();
    let client = PipewireTestClient::new(
        "isolated".to_string(),
        server,
        PipewireClient::new(
            Arc::new(Runtime::new().unwrap()),
            TEST_ENVIRONMENT.lock().unwrap().client_timeout.clone(),
        ).unwrap(),
    );
    client
}

#[fixture]
pub fn shared_client() -> PipewireTestClient {
    // Its seems that shared client, for some reason, raise
    // timeout error during init phase and create node object phase.
    // Give a bit of space between tests seem to mitigate that issue.
    thread::sleep(Duration::from_millis(10));
    let client = SHARED_CLIENT.lock().unwrap().clone();
    client
}

#[fixture]
pub fn client2(server_with_default_configuration: Arc<Server>) -> (PipewireTestClient, PipewireTestClient) {
    let server = server_with_default_configuration.clone();
    let runtime = Arc::new(Runtime::new().unwrap());
    let client_1 = PipewireClient::new(
        runtime.clone(),
        TEST_ENVIRONMENT.lock().unwrap().client_timeout.clone(),
    ).unwrap();
    let client_2 = PipewireClient::new(
        runtime.clone(),
        TEST_ENVIRONMENT.lock().unwrap().client_timeout.clone(),
    ).unwrap();
    (
        PipewireTestClient::new(
            "isolated_client_1".to_string(),
            server.clone(),
            client_1,
        ),
        PipewireTestClient::new(
            "isolated_client_2".to_string(),
            server.clone(),
            client_2,
        )
    )
}

#[fixture]
pub fn input_node(shared_client: PipewireTestClient) -> NodeInfoFixture {
    shared_client.create_input_node()
}

#[fixture]
pub fn output_node(shared_client: PipewireTestClient) -> NodeInfoFixture {
    shared_client.create_output_node()
}

#[fixture]
pub fn input_stream(input_node: NodeInfoFixture) -> StreamFixture {
    StreamFixture::new(input_node.client.clone(), input_node)
}

#[fixture]
pub fn output_stream(output_node: NodeInfoFixture) -> StreamFixture {
    StreamFixture::new(output_node.client.clone(), output_node)
}

#[fixture]
pub fn input_connected_stream(input_stream: StreamFixture) -> ConnectedStreamFixture {
    ConnectedStreamFixture::new(input_stream.client.clone(), input_stream)
}

#[fixture]
pub fn output_connected_stream(output_stream: StreamFixture) -> ConnectedStreamFixture {
    ConnectedStreamFixture::new(output_stream.client.clone(), output_stream)
}
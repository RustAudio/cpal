use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use crate::{Direction, NodeInfo, PipewireClient};
use rstest::fixture;
use crate::test_utils::server::{server_with_default_configuration, set_socket_env_vars, Container};

pub struct PipewireTestClient {
    server: Rc<RefCell<Container>>,
    client: PipewireClient,
}

impl PipewireTestClient {
    pub(self) fn new(server: Rc<RefCell<Container>>, client: PipewireClient) -> Self {
        Self {
            server,
            client,
        }
    }
}

impl Deref for PipewireTestClient {
    type Target = PipewireClient;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

#[fixture]
pub fn client(server_with_default_configuration: Container) -> PipewireTestClient {
    set_socket_env_vars(&server_with_default_configuration);
    PipewireTestClient::new(
        Rc::new(RefCell::new(server_with_default_configuration)),
        PipewireClient::new().unwrap()
    )
}

#[fixture]
pub fn client2(server_with_default_configuration: Container) -> (PipewireTestClient, PipewireTestClient) {
    set_socket_env_vars(&server_with_default_configuration);
    let server = Rc::new(RefCell::new(server_with_default_configuration));
    let client_1 = PipewireClient::new().unwrap();
    let client_2 = PipewireClient::new().unwrap();
    (
        PipewireTestClient::new(
            server.clone(),
            client_1
        ),
        PipewireTestClient::new(
            server.clone(),
            client_2
        )
    )
}

#[fixture]
pub fn input_nodes(client: PipewireTestClient) -> Vec<NodeInfo> {
    client.node().enumerate(Direction::Input).unwrap()
}

#[fixture]
pub fn output_nodes(client: PipewireTestClient) -> Vec<NodeInfo> {
    client.node().enumerate(Direction::Output).unwrap()
}

#[fixture]
pub fn default_input_node(input_nodes: Vec<NodeInfo>) -> NodeInfo {
    input_nodes.iter()
        .filter(|node| node.is_default)
        .last()
        .cloned()
        .unwrap()
}

#[fixture]
pub fn default_output_node(output_nodes: Vec<NodeInfo>) -> NodeInfo {
    output_nodes.iter()
        .filter(|node| node.is_default)
        .last()
        .cloned()
        .unwrap()
}
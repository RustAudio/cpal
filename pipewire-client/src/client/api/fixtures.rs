use std::panic::UnwindSafe;
use crate::{Direction, NodeInfo, PipewireClient};
use rstest::fixture;

#[fixture]
#[once]
pub(crate) fn client() -> PipewireClient {
    PipewireClient::new().unwrap()
}

#[fixture]
pub(crate) fn input_nodes(client: &PipewireClient) -> Vec<NodeInfo> {
    client.node().enumerate(Direction::Input).unwrap()
}

#[fixture]
pub(crate) fn output_nodes(client: &PipewireClient) -> Vec<NodeInfo> {
    client.node().enumerate(Direction::Output).unwrap()
}

#[fixture]
pub(crate) fn default_input_node(input_nodes: Vec<NodeInfo>) -> NodeInfo {
    input_nodes.iter()
        .filter(|node| node.is_default)
        .last()
        .cloned()
        .unwrap()
}

#[fixture]
pub(crate) fn default_output_node(output_nodes: Vec<NodeInfo>) -> NodeInfo {
    output_nodes.iter()
        .filter(|node| node.is_default)
        .last()
        .cloned()
        .unwrap()
}
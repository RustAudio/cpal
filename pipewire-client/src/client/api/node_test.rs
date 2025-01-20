use crate::states::NodeState;
use crate::test_utils::fixtures::client;
use crate::test_utils::fixtures::PipewireTestClient;
use crate::Direction;
use rstest::rstest;
use std::any::TypeId;

fn internal_enumerate(client: &PipewireTestClient, direction: Direction) {
    let nodes = client.node().enumerate(direction).unwrap();
    assert_eq!(false, nodes.is_empty());
    let default_node = nodes.iter()
        .filter(|node| node.is_default)
        .last();
    assert_eq!(true, default_node.is_some());
    let listeners = client.core().get_listeners().unwrap();
    let node_listeners = listeners.get(&TypeId::of::<NodeState>()).unwrap();
    for (_, listeners) in node_listeners {
        assert_eq!(0, listeners.len());
    }
}

fn internal_create(client: &PipewireTestClient, direction: Direction) {
    client.node()
        .create(
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            direction,
            2
        ).unwrap();
    let listeners = client.core().get_listeners().unwrap();
    let node_listeners = listeners.get(&TypeId::of::<NodeState>()).unwrap();
    for (_, listeners) in node_listeners {
        assert_eq!(0, listeners.len());
    }
}

#[rstest]
fn enumerate_input(
    client: PipewireTestClient,
) {
    internal_enumerate(&client, Direction::Input);
}

#[rstest]
fn enumerate_output(
    client: PipewireTestClient,
) {
    internal_enumerate(&client, Direction::Output);
}

#[rstest]
fn create_input(
    client: PipewireTestClient,
) {
    internal_create(&client, Direction::Input);
}

#[rstest]
fn create_output(
    client: PipewireTestClient,
) {
    internal_create(&client, Direction::Output);
}

#[rstest]
fn create_then_enumerate_input(
    client: PipewireTestClient,
) {
    let direction = Direction::Input;
    internal_create(&client, direction.clone());
    internal_enumerate(&client, direction.clone());
}

#[rstest]
fn create_then_enumerate_output(
    client: PipewireTestClient,
) {
    let direction = Direction::Output;
    internal_create(&client, direction.clone());
    internal_enumerate(&client, direction.clone());
}
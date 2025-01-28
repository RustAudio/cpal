use crate::states::NodeState;
use crate::test_utils::fixtures::{shared_client, PipewireTestClient};
use crate::Direction;
use rstest::rstest;
use serial_test::serial;
use std::any::TypeId;
use uuid::Uuid;

fn internal_enumerate(client: &PipewireTestClient, direction: Direction) -> Vec<String> {
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
    nodes.iter()
        .map(move |node| node.name.clone())
        .collect()
}

fn internal_create(client: &PipewireTestClient, direction: Direction) -> String {
    let node_name = Uuid::new_v4().to_string();
    client.node()
        .create(
            node_name.clone(),
            node_name.clone(),
            node_name.clone(),
            direction,
            2
        ).unwrap();
    let listeners = client.core().get_listeners().unwrap();
    let node_listeners = listeners.get(&TypeId::of::<NodeState>()).unwrap();
    for (_, listeners) in node_listeners {
        assert_eq!(0, listeners.len());
    }
    node_name
}

#[rstest]
#[serial]
fn enumerate_input(
    #[from(shared_client)] client: PipewireTestClient,
) {
    internal_enumerate(&client, Direction::Input);
}

#[rstest]
#[serial]
fn enumerate_output(
    #[from(shared_client)] client: PipewireTestClient,
) {
    internal_enumerate(&client, Direction::Output);
}

#[rstest]
#[serial]
fn create_input(
    #[from(shared_client)] client: PipewireTestClient,
) {
    internal_create(&client, Direction::Input);
}

#[rstest]
#[serial]
fn create_output(
    #[from(shared_client)] client: PipewireTestClient,
) {
    internal_create(&client, Direction::Output);
}

#[rstest]
#[serial]
fn create_twice_same_direction(
    #[from(shared_client)] client: PipewireTestClient,
) {
    let node_name = Uuid::new_v4().to_string();
    client.node()
        .create(
            node_name.clone(),
            node_name.clone(),
            node_name.clone(),
            Direction::Output,
            2
        ).unwrap();
    let error = client.node()
        .create(
            node_name.clone(),
            node_name.clone(),
            node_name.clone(),
            Direction::Output,
            2
        ).unwrap_err();
    assert_eq!(
        format!("Node with name({}) already exists", node_name),
        error.description
    )
}

#[rstest]
#[serial]
fn create_twice_different_direction(
    #[from(shared_client)] client: PipewireTestClient,
) {
    let node_name = Uuid::new_v4().to_string();
    client.node()
        .create(
            node_name.clone(),
            node_name.clone(),
            node_name.clone(),
            Direction::Input,
            2
        ).unwrap();
    client.node()
        .create(
            node_name.clone(),
            node_name.clone(),
            node_name.clone(),
            Direction::Output,
            2
        ).unwrap();
}

#[rstest]
#[serial]
fn create_then_enumerate_input(
    #[from(shared_client)] client: PipewireTestClient,
) {
    let direction = Direction::Input;
    let node = internal_create(&client, direction.clone());
    let nodes = internal_enumerate(&client, direction.clone());
    assert_eq!(true, nodes.contains(&node))
}

#[rstest]
#[serial]
fn create_then_enumerate_output(
    #[from(shared_client)] client: PipewireTestClient,
) {
    let direction = Direction::Output;
    let node = internal_create(&client, direction.clone());
    let nodes = internal_enumerate(&client, direction.clone());
    assert_eq!(true, nodes.contains(&node))
}
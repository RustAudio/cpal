use crate::client::api::fixtures::client;
use crate::{Direction, PipewireClient};
use rstest::rstest;
use serial_test::serial;

fn internal_enumerate(client: &PipewireClient, direction: Direction) {
    let nodes = client.node().enumerate(direction).unwrap();
    assert_eq!(false, nodes.is_empty());
    let default_node = nodes.iter()
        .filter(|node| node.is_default)
        .last();
    assert_eq!(true, default_node.is_some());
}

fn internal_create(client: &PipewireClient, direction: Direction) {
    client.node()
        .create(
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            direction,
            2
        ).unwrap();
}

#[rstest]
#[case::input(Direction::Input)]
#[case::output(Direction::Output)]
#[serial]
fn enumerate(
    client: &PipewireClient,
    #[case] direction: Direction
) {
    internal_enumerate(&client, direction);
}

#[rstest]
#[case::input(Direction::Input)]
#[case::output(Direction::Output)]
#[serial]
fn create(
    client: &PipewireClient,
    #[case] direction: Direction
) {
    internal_create(&client, direction);
}

#[rstest]
#[case::input(Direction::Input)]
#[case::output(Direction::Output)]
#[serial]
fn create_then_enumerate(
    client: &PipewireClient,
    #[case] direction: Direction
) {
    internal_create(&client, direction.clone());
    internal_enumerate(&client, direction.clone());
}
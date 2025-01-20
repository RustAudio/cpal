use crate::listeners::ListenerControlFlow;
use crate::states::StreamState;
use crate::test_utils::fixtures::client;
use crate::test_utils::fixtures::PipewireTestClient;
use crate::{Direction, NodeInfo};
use rstest::rstest;
use serial_test::serial;
use std::any::TypeId;

fn internal_create<F>(
    client: &PipewireTestClient,
    node: NodeInfo,
    direction: Direction,
    callback: F,
) -> String where
    F: FnMut(&mut ListenerControlFlow, pipewire::buffer::Buffer) + Send + 'static
{
    client.stream()
        .create(
            node.id,
            direction,
            node.format.clone().into(),
            callback
        )
        .unwrap()
}

fn internal_delete(
    client: &PipewireTestClient,
    stream: &String
) {
    client.stream()
        .delete(stream.clone())
        .unwrap()
}

fn internal_create_connected<F>(
    client: &PipewireTestClient,
    node: NodeInfo,
    direction: Direction,
    callback: F,
) -> String where
    F: FnMut(&mut ListenerControlFlow, pipewire::buffer::Buffer) + Send + 'static
{
    let stream = client.stream()
        .create(
            node.id,
            direction,
            node.format.clone().into(),
            callback
        )
        .unwrap();
    client.stream().connect(stream.clone()).unwrap();
    stream
}

fn abstract_create(
    client: &PipewireTestClient,
    direction: Direction
) -> String {
    let stream = internal_create(
        &client,
        match direction {
            Direction::Input => client.default_input_node().clone(),
            Direction::Output => client.default_output_node().clone()
        },
        direction.clone(),
        move |control_flow, _| {
            assert!(true);
            control_flow.release();
        }
    );
    match direction {
        Direction::Input => assert_eq!(true, stream.ends_with(".stream_input")),
        Direction::Output => assert_eq!(true, stream.ends_with(".stream_output"))
    };
    let listeners = client.core().get_listeners().unwrap();
    let stream_listeners = listeners.get(&TypeId::of::<StreamState>()).unwrap();
    for (_, listeners) in stream_listeners {
        // Expect one listener since we created a stream object with our callback set
        assert_eq!(1, listeners.len());
    }
    stream
}

#[rstest]
#[serial]
fn create_input(
    client: PipewireTestClient,
) {
    let direction = Direction::Input;
    abstract_create(&client, direction);
}

#[rstest]
#[serial]
fn create_output(
    client: PipewireTestClient,
) {
    let direction = Direction::Output;
    abstract_create(&client, direction);
}

#[rstest]
#[serial]
fn delete_input(
    client: PipewireTestClient,
) {
    let direction = Direction::Input;
    let stream = abstract_create(&client, direction);
    client.stream().delete(stream).unwrap();
    let listeners = client.core().get_listeners().unwrap();
    let stream_listeners = listeners.get(&TypeId::of::<StreamState>()).unwrap();
    for (_, listeners) in stream_listeners {
        assert_eq!(0, listeners.len());
    }
}

#[rstest]
#[serial]
fn delete_output(
    client: PipewireTestClient,
) {
    let direction = Direction::Output;
    let stream = abstract_create(&client, direction);
    client.stream().delete(stream).unwrap()
}

fn abstract_connect(
    client: &PipewireTestClient,
    direction: Direction
) {
    let stream = internal_create(
        &client,
        match direction {
            Direction::Input => client.default_input_node().clone(),
            Direction::Output => client.default_output_node().clone()
        },
        direction.clone(),
        move |control_flow, mut buffer| {
            let data = buffer.datas_mut();
            let data = &mut data[0];
            let data = data.data().unwrap();
            assert_eq!(true, data.len() > 0);
            control_flow.release();
        }
    );
    client.stream().connect(stream.clone()).ok().unwrap();
    // Wait a bit to test if stream callback will panic
    std::thread::sleep(std::time::Duration::from_millis(1 * 1000));
}

#[rstest]
#[serial]
fn connect_input(
    client: PipewireTestClient,
) {
    let direction = Direction::Input;
    abstract_connect(&client, direction);
}

#[rstest]
#[serial]
fn connect_output(
    client: PipewireTestClient,
) {
    let direction = Direction::Output;
    abstract_connect(&client, direction);
}

fn abstract_disconnect(
    client: &PipewireTestClient,
    direction: Direction
) {
    let stream = internal_create_connected(
        &client,
        match direction {
            Direction::Input => client.default_input_node().clone(),
            Direction::Output => client.default_output_node().clone()
        },
        direction.clone(),
        move |control_flow, _| {
            assert!(true);
            control_flow.release();
        }
    );
    client.stream().disconnect(stream.clone()).unwrap();
}

#[rstest]
#[serial]
fn disconnect_input(
    client: PipewireTestClient,
) {
    let direction = Direction::Input;
    abstract_disconnect(&client, direction);
}

#[rstest]
#[serial]
fn disconnect_output(
    client: PipewireTestClient,
) {
    let direction = Direction::Output;
    abstract_disconnect(&client, direction);
}
use crate::listeners::ListenerControlFlow;
use crate::states::StreamState;
use crate::test_utils::fixtures::{input_connected_stream, input_node, input_stream, output_connected_stream, output_node, output_stream, shared_client, ConnectedStreamFixture, NodeInfoFixture, PipewireTestClient, StreamFixture};
use crate::{Direction, PipewireClient};
use rstest::rstest;
use serial_test::serial;
use std::any::TypeId;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use crate::client::api::StreamApi;
use crate::client::CoreApi;

fn assert_listeners(client: &CoreApi, stream_name: &String, expected_listener: u32) {
    let listeners = client.get_listeners().unwrap();
    let stream_listeners = listeners.get(&TypeId::of::<StreamState>()).unwrap().iter()
        .find_map(move |(key, listeners)| {
            if key == stream_name {
                Some(listeners)
            }
            else {
                None
            }
        })
        .unwrap();
    assert_eq!(expected_listener as usize, stream_listeners.len());
}

fn internal_create<F>(
    client: &StreamApi,
    node: &NodeInfoFixture,
    direction: Direction,
    callback: F,
) -> String
where
    F: FnMut(&mut ListenerControlFlow, pipewire::buffer::Buffer) + Send + 'static
{
    let stream_name = client
        .create(
            node.id,
            direction,
            node.format.clone().into(),
            callback
        )
        .unwrap();
    stream_name
}

fn abstract_create(
    client: &PipewireClient,
    node: &NodeInfoFixture,
    direction: Direction
) {
    let stream = internal_create(
        &client.stream(),
        node,
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
    assert_listeners(client.core(), &stream, 1);
}

#[rstest]
#[serial]
fn create_input(
    #[from(input_node)] node: NodeInfoFixture
) {
    let direction = Direction::Input;
    abstract_create(&node.client(), &node, direction);
}

#[rstest]
#[serial]
fn create_output(
    #[from(output_node)] node: NodeInfoFixture
) {
    let direction = Direction::Output;
    abstract_create(&node.client(), &node, direction);
}

#[rstest]
#[serial]
fn create_twice(
    #[from(output_node)] node: NodeInfoFixture
) {
    let direction = Direction::Output;
    let stream = node.client().stream()
        .create(
            node.id,
            direction.clone(),
            node.format.clone().into(),
           move |_, _| {} 
        )
        .unwrap();
    let error = node.client().stream()
        .create(
            node.id,
            direction.clone(),
            node.format.clone().into(),
            move |_, _| {}
        )
        .unwrap_err();
    assert_eq!(
        format!("Stream with name({}) already exists", stream),
        error.description
    );
    assert_listeners(node.client().core(), &stream, 1);
}

#[rstest]
#[serial]
fn delete_input(
    #[from(input_stream)] stream: StreamFixture
) {
    stream.delete().unwrap();
}

#[rstest]
#[serial]
fn delete_output(
    #[from(output_stream)] stream: StreamFixture
) {
    stream.delete().unwrap();
}

#[rstest]
#[serial]
fn delete_when_not_exists(
    #[from(shared_client)] client: PipewireTestClient,
) {
    let stream = "not_existing_stream".to_string();
    let error = client.stream().delete(stream.clone()).unwrap_err();
    assert_eq!(
        format!("Stream with name({}) not found", stream),
        error.description
    )
}

#[rstest]
#[serial]
fn delete_twice(
    #[from(output_stream)] stream: StreamFixture
) {
    stream.delete().unwrap();
    let error = stream.delete().unwrap_err();
    assert_eq!(
        format!("Stream with name({}) not found", stream),
        error.description
    )
}

#[rstest]
#[serial]
fn connect_input(
    #[from(input_stream)] stream: StreamFixture
) {
    stream.connect().unwrap();
    assert_listeners(stream.client().core(), &stream, 1);
}

#[rstest]
#[serial]
fn connect_output(
    #[from(output_stream)] stream: StreamFixture
) {
    stream.connect().unwrap();
    assert_listeners(stream.client().core(), &stream, 1);
}

#[rstest]
#[serial]
fn connect_twice(
    #[from(output_connected_stream)] stream: ConnectedStreamFixture
) {
    let error = stream.connect().unwrap_err();
    assert_eq!(
        format!("Stream {} is already connected", stream), 
        error.description
    )
}

#[rstest]
#[serial]
fn disconnect_input(
    #[from(input_connected_stream)] stream: ConnectedStreamFixture
) {
    stream.disconnect().unwrap();
    assert_listeners(stream.client().core(), &stream, 1);
}

#[rstest]
#[serial]
fn disconnect_output(
    #[from(output_connected_stream)] stream: ConnectedStreamFixture
) {
    stream.disconnect().unwrap();
    assert_listeners(stream.client().core(), &stream, 1);
}

#[rstest]
#[serial]
fn disconnect_when_not_connected(
    #[from(output_stream)] stream: StreamFixture
) {
    let error = stream.disconnect().unwrap_err();
    assert_eq!(
        format!("Stream {} is not connected", stream),
        error.description
    )
}

#[rstest]
#[serial]
fn disconnect_twice(
    #[from(output_connected_stream)] stream: ConnectedStreamFixture
) {
    stream.disconnect().unwrap();
    let error = stream.disconnect().unwrap_err();
    assert_eq!(
        format!("Stream {} is not connected", stream),
        error.description
    )
}
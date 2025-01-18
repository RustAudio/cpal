use std::panic;
use crate::client::api::fixtures::{client, default_input_node, default_output_node};
use crate::{Direction, NodeInfo, PipewireClient};
use rstest::rstest;
use serial_test::serial;

fn internal_create<F: FnMut(pipewire::buffer::Buffer) + Send + 'static>(
    client: &PipewireClient,
    node: NodeInfo,
    direction: Direction,
    callback: F,
) -> String {
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
    client: &PipewireClient,
    stream: &String
) {
    client.stream()
        .delete(stream.clone())
        .unwrap()
}

fn internal_create_connected<F: FnMut(pipewire::buffer::Buffer) + Send + 'static>(
    client: &PipewireClient,
    node: NodeInfo,
    direction: Direction,
    callback: F,
) -> String {
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

struct StreamTest<S, T, D>
where
    S: Fn() -> String,
    T: Fn(&String) -> (),
    D: Fn(&String),
{
    setup: S,
    test: T,
    teardown: D,
    stream_name: Option<String>
}

impl <S, T, D> StreamTest<S, T, D>
where
    S: Fn() -> String,
    T: Fn(&String) -> (),
    D: Fn(&String),
{
    fn new(setup: S, test: T, teardown: D) -> Self {
        Self {
            setup,
            test,
            teardown,
            stream_name: None,
        }
    }
    
    fn run(&mut self) {
        self.stream_name = Some((self.setup)());
        (self.test)(self.stream_name.as_ref().unwrap());
    }
}

impl <S, T, D> Drop for StreamTest<S, T, D>
where
    S: Fn() -> String,
    T: Fn(&String) -> (),
    D: Fn(&String),
{
    fn drop(&mut self) {
        (self.teardown)(self.stream_name.as_ref().unwrap())
    }
}

#[rstest]
#[case::input(Direction::Input)]
#[case::output(Direction::Output)]
#[serial]
fn create(
    client: &PipewireClient,
    default_input_node: NodeInfo,
    default_output_node: NodeInfo,
    #[case] direction: Direction
) {
    let mut test = StreamTest::new(
        || {
            internal_create(
                &client,
                match direction {
                    Direction::Input => default_input_node.clone(),
                    Direction::Output => default_output_node.clone()
                },
                direction.clone(),
                move |_| {
                    assert!(true);
                }
            )
        },
        |stream| {
            match direction {
                Direction::Input => assert_eq!(true, stream.ends_with(".stream_input")),
                Direction::Output => assert_eq!(true, stream.ends_with(".stream_output"))
            };
        },
        |stream| {
            internal_delete(&client, stream);
        }
    );
    test.run();
}

#[rstest]
#[case::input(Direction::Input)]
#[case::output(Direction::Output)]
#[serial]
fn connect(
    client: &PipewireClient,
    default_input_node: NodeInfo,
    default_output_node: NodeInfo,
    #[case] direction: Direction
) {
    let mut test = StreamTest::new(
        || {
            internal_create(
                &client,
                match direction {
                    Direction::Input => default_input_node.clone(),
                    Direction::Output => default_output_node.clone()
                },
                direction.clone(),
                move |mut buffer| {
                    let data = buffer.datas_mut();
                    let data = &mut data[0];
                    let data = data.data().unwrap();
                    assert_eq!(true, data.len() > 0);
                }
            )
        },
        |stream| {
            client.stream().connect(stream.clone()).ok().unwrap();
            // Wait a bit to test if stream callback will panic
            std::thread::sleep(std::time::Duration::from_millis(1 * 1000));
        },
        |stream| {
            internal_delete(&client, stream);
        }
    );
    test.run();
}

#[rstest]
#[case::input(Direction::Input)]
#[case::output(Direction::Output)]
#[serial]
fn disconnect(
    client: &PipewireClient,
    default_input_node: NodeInfo,
    default_output_node: NodeInfo,
    #[case] direction: Direction
) {
    let mut test = StreamTest::new(
        || {
            internal_create_connected(
                &client,
                match direction {
                    Direction::Input => default_input_node.clone(),
                    Direction::Output => default_output_node.clone()
                },
                direction.clone(),
                move |_| {
                    assert!(true);
                }
            )
        },
        |stream| {
            client.stream().disconnect(stream.clone()).unwrap();
        },
        |stream| {
            internal_delete(&client, stream);
        }
    );
    test.run();
}
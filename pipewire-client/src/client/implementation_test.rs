use std::sync::atomic::Ordering;
use rstest::rstest;
use serial_test::serial;
use crate::client::implementation::CLIENT_INDEX;
use crate::{Direction, PipewireClient};

#[rstest]
#[serial]
pub fn all() {
    for _ in 0..100 {
        name();
        quit();
        settings();
        default_audio_nodes();
        create_node();
        create_node_then_enumerate_nodes();
        create_stream();
        enumerate_nodes();
    }
}

#[rstest]
#[serial]
pub fn name() {
    let client_1 = PipewireClient::new().unwrap();
    assert_eq!(format!("cpal-client-{}", CLIENT_INDEX.load(Ordering::SeqCst) - 1), client_1.name);
    let client_2 = PipewireClient::new().unwrap();
    assert_eq!(format!("cpal-client-{}", CLIENT_INDEX.load(Ordering::SeqCst) - 1), client_2.name);
}

#[rstest]
#[serial]
fn quit() {
    let client = PipewireClient::new().unwrap();
    client.quit();
}

#[rstest]
#[serial]
fn settings() {
    let client = PipewireClient::new().unwrap();
    let response = client.settings();
    assert!(
        response.is_ok(),
        "Should send settings message without errors"
    );
    let settings = response.unwrap();
    assert_eq!(true, settings.sample_rate > u32::default());
    assert_eq!(true, settings.default_buffer_size > u32::default());
    assert_eq!(true, settings.min_buffer_size > u32::default());
    assert_eq!(true, settings.max_buffer_size > u32::default());
    assert_eq!(true, settings.allowed_sample_rates[0] > u32::default());
}

#[rstest]
#[serial]
fn default_audio_nodes() {
    let client = PipewireClient::new().unwrap();
    let response = client.default_audio_nodes();
    assert!(
        response.is_ok(),
        "Should send default audio nodes message without errors"
    );
    let default_audio_nodes = response.unwrap();
    assert_eq!(false, default_audio_nodes.sink.is_empty());
    assert_eq!(false, default_audio_nodes.source.is_empty());
}

#[rstest]
#[serial]
fn create_node() {
    let client = PipewireClient::new().unwrap();
    let response = client.create_node(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        Direction::Output,
        2
    );
    assert!(
        response.is_ok(),
        "Should send create node message without errors"
    );
}

#[rstest]
#[serial]
fn create_node_then_enumerate_nodes() {
    let client = PipewireClient::new().unwrap();
    let response = client.create_node(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        Direction::Output,
        2
    );
    assert!(
        response.is_ok(),
        "Should send create node message without errors"
    );
    let response = client.enumerate_nodes(Direction::Output);
    assert!(
        response.is_ok(),
        "Should send enumerate devices message without errors"
    );
    let nodes = response.unwrap();
    assert_eq!(false, nodes.is_empty());
    let default_node = nodes.iter()
        .filter(|node| node.is_default)
        .last();
    assert_eq!(true, default_node.is_some());
}

#[rstest]
#[serial]
fn create_stream() {
    let client = PipewireClient::new().unwrap();
    let response = client.enumerate_nodes(Direction::Output).unwrap();
    let default_node = response.iter()
        .filter(|node| node.is_default)
        .last()
        .unwrap();
    let response = client.create_stream(
        default_node.id,
        Direction::Output,
        default_node.format.clone().into(),
        move |mut buffer| {
            let data = buffer.datas_mut();
            let data = &mut data[0];
            let data = data.data().unwrap();
            assert_eq!(true, data.len() > 0);
        }
    );
    assert!(
        response.is_ok(),
        "Should send create stream message without errors"
    );
    let stream_name = response.ok().unwrap();
    let response = client.connect_stream(stream_name);
    std::thread::sleep(std::time::Duration::from_millis(1 * 1000));
    assert!(
        response.is_ok(),
        "Should send connect stream message without errors"
    );
}

#[rstest]
#[serial]
fn enumerate_nodes() {
    let client = PipewireClient::new().unwrap();
    let response = client.enumerate_nodes(Direction::Output);
    assert!(
        response.is_ok(),
        "Should send enumerate devices message without errors"
    );
    let nodes = response.unwrap();
    assert_eq!(false, nodes.is_empty());
    let default_node = nodes.iter()
        .filter(|node| node.is_default)
        .last();
    assert_eq!(true, default_node.is_some());
}
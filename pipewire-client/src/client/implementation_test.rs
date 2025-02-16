use crate::client::implementation::{CLIENT_INDEX, CLIENT_NAME_PREFIX};
use crate::states::{MetadataState, NodeState};
use crate::test_utils::fixtures::{client2, shared_client, PipewireTestClient};
use crate::PipewireClient;
use rstest::rstest;
use serial_test::serial;
use std::any::TypeId;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::runtime::Runtime;
use pipewire_test_utils::environment::TEST_ENVIRONMENT;
use pipewire_test_utils::server::{server_with_default_configuration, server_without_node, server_without_session_manager, Server};
use crate::listeners::PipewireCoreSync;

#[rstest]
#[serial]
pub fn names(
    #[from(client2)] (client_1, client_2): (PipewireTestClient, PipewireTestClient)
) {
    let client_1_index = client_1.name.replace(format!("{}-", CLIENT_NAME_PREFIX).as_str(), "")
        .parse::<u32>()
        .unwrap();
    assert_eq!(format!("{}-{}", CLIENT_NAME_PREFIX, client_1_index), client_1.name);
    assert_eq!(format!("{}-{}", CLIENT_NAME_PREFIX, client_1_index + 1), client_2.name);
}

#[rstest]
#[serial]
#[ignore]
fn init100(#[from(server_with_default_configuration)] _server: Arc<Server>) {
    for index in 0..100 {
        thread::sleep(Duration::from_millis(10));
        println!("Init client: {}", index);
        let _ = PipewireClient::new(
            Arc::new(Runtime::new().unwrap()),
            TEST_ENVIRONMENT.lock().unwrap().client_timeout.clone(),
        ).unwrap();
        assert_eq!(index + 1, CLIENT_INDEX.load(std::sync::atomic::Ordering::SeqCst));
    }
}

#[rstest]
#[serial]
pub fn with_default_configuration(#[from(shared_client)] client: PipewireTestClient) {
    let listeners = client.core().get_listeners().unwrap();
    let core_listeners = listeners.get(&TypeId::of::<PipewireCoreSync>()).unwrap();
    let metadata_listeners = listeners.get(&TypeId::of::<MetadataState>()).unwrap();
    let nodes_listeners = listeners.get(&TypeId::of::<NodeState>()).unwrap();
    // No need to check stream listeners since we had to create them in first place (i.e. after client init phases).
    for (_, listeners) in core_listeners {
        assert_eq!(0, listeners.len());
    }
    for (_, listeners) in metadata_listeners {
        assert_eq!(0, listeners.len());
    }
    for (_, listeners) in nodes_listeners {
        assert_eq!(0, listeners.len());
    }
}

#[rstest]
#[serial]
pub fn without_session_manager(#[from(server_without_session_manager)] _server: Arc<Server>) {
    let error = PipewireClient::new(
        Arc::new(Runtime::new().unwrap()),
        TEST_ENVIRONMENT.lock().unwrap().client_timeout.clone(),
    ).unwrap_err();
    assert_eq!(true, error.description.contains("No session manager registered"))
}

#[rstest]
#[serial]
pub fn without_node(#[from(server_without_node)] _server: Arc<Server>) {
    let error = PipewireClient::new(
        Arc::new(Runtime::new().unwrap()),
        TEST_ENVIRONMENT.lock().unwrap().client_timeout.clone(),
    ).unwrap_err();
    assert_eq!("Post initialization error: Zero node registered", error.description)
}
use crate::client::implementation::CLIENT_NAME_PREFIX;
use crate::test_utils::fixtures::{client2, PipewireTestClient};
use crate::test_utils::server::{server_with_default_configuration, server_without_node, server_without_session_manager, set_socket_env_vars, Container};
use crate::PipewireClient;
use rstest::rstest;
use serial_test::serial;

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
pub fn with_default_configuration(server_with_default_configuration: Container) {
    set_socket_env_vars(&server_with_default_configuration);
    let _ = PipewireClient::new().unwrap();
}

#[rstest]
#[serial]
pub fn without_session_manager(server_without_session_manager: Container) {
    set_socket_env_vars(&server_without_session_manager);
    let error = PipewireClient::new().unwrap_err();
    assert_eq!(true, error.description.contains("No session manager registered"))
}

#[rstest]
#[serial]
pub fn without_node(server_without_node: Container) {
    set_socket_env_vars(&server_without_node);
    let error = PipewireClient::new().unwrap_err();
    assert_eq!("Post initialization error: Zero node registered", error.description)
}
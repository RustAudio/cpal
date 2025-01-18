use crate::client::implementation::CLIENT_INDEX;
use crate::PipewireClient;
use rstest::rstest;
use serial_test::serial;
use std::sync::atomic::Ordering;

#[rstest]
#[serial]
pub fn name() {
    let client_1 = PipewireClient::new().unwrap();
    assert_eq!(format!("cpal-client-{}", CLIENT_INDEX.load(Ordering::SeqCst) - 1), client_1.name);
    let client_2 = PipewireClient::new().unwrap();
    assert_eq!(format!("cpal-client-{}", CLIENT_INDEX.load(Ordering::SeqCst) - 1), client_2.name);
}
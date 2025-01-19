use crate::test_utils::fixtures::{client, PipewireTestClient};
use rstest::rstest;
use serial_test::serial;

#[rstest]
#[serial]
fn quit(client: PipewireTestClient) {
    client.core().quit();
}

#[rstest]
#[serial]
pub fn settings(client: PipewireTestClient) {
    let settings = client.core().get_settings().unwrap();
    assert_eq!(true, settings.sample_rate > u32::default());
    assert_eq!(true, settings.default_buffer_size > u32::default());
    assert_eq!(true, settings.min_buffer_size > u32::default());
    assert_eq!(true, settings.max_buffer_size > u32::default());
    assert_eq!(true, settings.allowed_sample_rates[0] > u32::default());
}

#[rstest]
#[serial]
pub fn default_audio_nodes(client: PipewireTestClient) {
    let default_audio_nodes = client.core().get_default_audio_nodes().unwrap();
    assert_eq!(false, default_audio_nodes.sink.is_empty());
    assert_eq!(false, default_audio_nodes.source.is_empty());
}
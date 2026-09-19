/*
Demonstrates the app-controlled macOS system audio recording permission flow.

Run with:
    cargo run --example macos_system_audio_permission
*/

#[cfg(target_os = "macos")]
fn main() {
    if cpal::platform::check_system_audio_permission() {
        println!("System audio recording permission is already granted");
        return;
    }

    println!("Requesting system audio recording permission...");
    if cpal::platform::request_system_audio_permission() {
        println!("System audio recording permission granted");
    } else {
        println!(
            "System audio recording permission denied or unavailable; opening System Settings"
        );
        cpal::platform::open_system_audio_settings();
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example is only available on macOS");
}

extern crate cpal;

fn main() {
    println!("Default Output Device:\n  {:?}", cpal::default_output_device().map(|e| e.name()));

    let devices = cpal::devices();
    println!("Devices: ");
    for (device_index, device) in devices.enumerate() {
        println!("{}. device \"{}\" Output stream formats: ",
                 device_index + 1,
                 device.name());

        let output_formats = match device.supported_output_formats() {
            Ok(f) => f,
            Err(e) => {
                println!("Error: {:?}", e);
                continue;
            },
        };

        for (format_index, format) in output_formats.enumerate() {
            println!("{}.{}. {:?}", device_index + 1, format_index + 1, format);
        }
    }
}

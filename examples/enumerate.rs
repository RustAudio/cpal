extern crate cpal;

fn main() {
    println!(
        "Default Input Device:\n  {:?}",
        cpal::default_input_device().map(|e| e.name())
    );
    println!(
        "Default Output Device:\n  {:?}",
        cpal::default_output_device().map(|e| e.name())
    );

    let devices = cpal::devices();
    println!("Devices: ");
    for (device_index, device) in devices.enumerate() {
        println!("{}. \"{}\"", device_index + 1, device.name());

        // Input formats
        if let Ok(fmt) = device.default_input_format() {
            println!("  Default input stream format:\n    {:?}", fmt);
        }
        let mut input_formats = match device.supported_input_formats() {
            Ok(f) => f.peekable(),
            Err(e) => {
                println!("Error: {:?}", e);
                continue;
            }
        };
        if input_formats.peek().is_some() {
            println!("  All supported input stream formats:");
            for (format_index, format) in input_formats.enumerate() {
                println!(
                    "    {}.{}. {:?}",
                    device_index + 1,
                    format_index + 1,
                    format
                );
            }
        }

        // Output formats
        if let Ok(fmt) = device.default_output_format() {
            println!("  Default output stream format:\n    {:?}", fmt);
        }
        let mut output_formats = match device.supported_output_formats() {
            Ok(f) => f.peekable(),
            Err(e) => {
                println!("Error: {:?}", e);
                continue;
            }
        };
        if output_formats.peek().is_some() {
            println!("  All supported output stream formats:");
            for (format_index, format) in output_formats.enumerate() {
                println!(
                    "    {}.{}. {:?}",
                    device_index + 1,
                    format_index + 1,
                    format
                );
            }
        }
    }
}

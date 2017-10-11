extern crate cpal;

fn main() {
    let endpoints = cpal::endpoints();
    
    println!("Endpoints: ");
    for (endpoint_index, endpoint) in endpoints.enumerate() {
        println!("{}. Endpoint \"{}\" Audio formats: ", endpoint_index + 1, endpoint.name());

        let formats = match endpoint.supported_formats() {
            Ok(f) => f,
            Err(e) => { println!("Error: {:?}", e); continue; }
        };

        for (format_index, format) in formats.enumerate() {
            println!("{}.{}. {:?}", endpoint_index + 1, format_index + 1, format);
        }
    }
}

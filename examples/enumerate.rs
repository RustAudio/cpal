extern crate cpal;

use cpal::*;

fn main() {
    let endpoints = cpal::get_endpoints_list();
    
    println!("Endpoints: ");
    for (endpoint_index, endpoint) in endpoints.enumerate() {
        println!("{}. Endpoint \"{}\" Audio formats: ", endpoint_index + 1, endpoint.get_name());

        let formats = match endpoint.get_supported_formats_list() {
            Ok(f) => f,
            Err(e) => { println!("Error: {:?}", e); continue; }
        };

        for (format_index, format) in formats.enumerate() {
            println!("{}.{}. {:?}", endpoint_index + 1, format_index + 1, format);
        }
    }
}

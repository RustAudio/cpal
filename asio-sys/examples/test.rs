extern crate asio_sys as sys;

fn main() {
    let driver_list = sys::get_driver_list();

    for driver in &driver_list{
        println!("Driver: {}", driver);
    }

    if driver_list.len() > 0 {
        match sys::get_channels(& driver_list[0]); {
            Ok(channels) => println!("Channels: {:?}", channels);,
            Err(e) => println("Error retrieving channels: {}", e),
        }
    }
    
}

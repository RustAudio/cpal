extern crate asio_sys as sys;

fn main() {
    let driver_list = sys::get_driver_names();

    for driver in driver_list{
        println!(driver);
    }
    
}

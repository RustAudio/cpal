extern crate asio_sys as sys;

fn main() {
    let driver_list = sys::get_driver_list();

    for driver in &driver_list {
        println!("Driver: {}", driver);

        let driver = sys::Drivers::load(driver).expect("failed to load drivers");
        println!("  Channels: {:?}", driver.get_channels());
        println!("  Sample rate: {:?}", driver.get_sample_rate());
    }
}

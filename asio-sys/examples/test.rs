extern crate asio_sys as sys;

fn main() {
    let asio = sys::Asio::new();
    for driver in asio.driver_names() {
        println!("Driver: {}", driver);
        let driver = asio.load_driver(&driver).expect("failed to load drivers");
        println!(
            "  Channels: {:?}",
            driver.channels().expect("failed to get channels")
        );
        println!(
            "  Sample rate: {:?}",
            driver.sample_rate().expect("failed to get sample rate")
        );
    }
}

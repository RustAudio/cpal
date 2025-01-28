extern crate cargo;
extern crate cargo_metadata;
extern crate indexmap;
extern crate itertools;
extern crate quote;
extern crate syn;

mod build_modules;

use build_modules::format;
use build_modules::utils::map_package_info;


fn main() {
    let package = map_package_info();
    format::generate_enums(&package.src_path, &package.build_path, &package.features);
}


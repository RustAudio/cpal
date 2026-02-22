use pipewire::sys;
use std::ffi::CStr;
use std::sync::LazyLock;

// unfortunately we have to take two args as concat_idents! is in experimental
macro_rules! key_constant {
    ($name:ident, $pw_symbol:ident, #[doc = $doc:expr]) => {
        #[doc = $doc]
        pub static $name: LazyLock<&'static str> = LazyLock::new(|| unsafe {
            CStr::from_bytes_with_nul_unchecked(sys::$pw_symbol)
                .to_str()
                .unwrap()
        });
    };
}

key_constant!(METADATA_NAME, PW_KEY_METADATA_NAME,
    /// METADATA_NAME
);

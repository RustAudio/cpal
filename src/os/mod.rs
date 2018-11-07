/// The cpal::os module provides operating-system-specific 
/// functionality. If you are using this module within a 
/// cross-platform project, you may wish to use 
/// cfg(target_os = "<os_name>") to ensure that you only 
/// use the OS-specific items when compiling for that OS.
#[cfg(target_os = "windows")]
pub mod windows;

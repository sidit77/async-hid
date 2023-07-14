#[cfg(target_os = "windows")]
mod winrt;
#[cfg(target_os = "windows")]
pub use winrt::{enumerate, open, BackendDevice, BackendDeviceId, BackendError};

#[cfg(target_os = "linux")]
mod hidraw;
#[cfg(target_os = "linux")]
pub use hidraw::{enumerate, open, BackendDevice, BackendDeviceId, BackendError};

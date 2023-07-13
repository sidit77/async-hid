
#[cfg(target_os = "windows")]
mod winrt;
#[cfg(target_os = "windows")]
pub use winrt::{BackendError, BackendDeviceId, BackendDevice, enumerate, open};

#[cfg(target_os = "linux")]
mod hidraw;
#[cfg(target_os = "linux")]
pub use hidraw::{BackendError, BackendDeviceId, BackendDevice, enumerate, open};

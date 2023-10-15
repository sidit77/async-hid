#[cfg(target_os = "windows")]
mod winrt;
#[cfg(target_os = "windows")]
pub use winrt::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};

#[cfg(target_os = "linux")]
mod hidraw;
#[cfg(target_os = "linux")]
pub use hidraw::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};

#[cfg(target_os = "macos")]
mod iohidmanager;
#[cfg(target_os = "macos")]
pub use iohidmanager::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};

#[cfg(target_arch = "wasm32")]
mod webhid;
#[cfg(target_arch = "wasm32")]
pub use webhid::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};
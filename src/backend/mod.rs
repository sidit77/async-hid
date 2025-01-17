#[cfg(all(target_os = "windows", feature = "win32"))]
mod win32;
#[cfg(all(target_os = "windows", feature = "win32"))]
pub use win32::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};

#[cfg(all(target_os = "windows", feature = "winrt"))]
mod winrt;
#[cfg(all(target_os = "windows", feature = "winrt"))]
pub use winrt::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};

#[cfg(all(feature = "win32", feature = "winrt"))]
compile_error!("Only win32 or winrt can be active at the same time");


#[cfg(target_os = "linux")]
mod hidraw;
#[cfg(target_os = "linux")]
pub use hidraw::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};


#[cfg(target_os = "macos")]
mod iohidmanager;
#[cfg(target_os = "macos")]
pub use iohidmanager::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};

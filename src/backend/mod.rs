use crate::{DeviceInfo, HidResult};
use std::fmt::{Debug, Display};
use std::future::Future;
use std::hash::Hash;
use futures_core::Stream;
use crate::traits::{AsyncHidRead, AsyncHidWrite};

#[cfg(all(target_os = "windows", feature = "win32"))]
mod win32;

#[cfg(all(target_os = "windows", feature = "winrt"))]
mod winrt;

#[cfg(target_os = "linux")]
mod hidraw;
#[cfg(target_os = "linux")]
pub use hidraw::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};


#[cfg(target_os = "macos")]
mod iohidmanager;
#[cfg(target_os = "macos")]
pub use iohidmanager::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};


pub trait Backend: Sized {
    type Error: Debug + Display + Send + Sync;
    type DeviceId: Debug + PartialEq + Eq + Clone + Hash + Send + Sync;
    type Reader: AsyncHidRead + Send + Sync;
    type Writer: AsyncHidWrite + Send + Sync;

    fn enumerate() -> impl Future<Output = HidResult<impl Stream<Item = DeviceInfo<Self>> + Unpin + Send, Self>> + Send;

    fn open(id: &Self::DeviceId, read: bool, write: bool) -> impl Future<Output = HidResult<(Option<Self::Reader>, Option<Self::Writer>), Self>> + Send;

}

#[cfg(all(target_os = "windows", feature = "win32"))]
pub type DefaultBackend = win32::Win32Backend;

#[cfg(all(target_os = "windows", feature = "winrt", not(feature = "win32")))]
pub type DefaultBackend = winrt::WinRtBackend;


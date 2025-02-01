use std::fmt::{Debug, Display};
use std::future::Future;
use futures_core::Stream;
use crate::traits::{AsyncHidRead, AsyncHidWrite};

#[cfg(all(target_os = "windows", feature = "win32"))]
mod win32;

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
use crate::{DeviceInfo, HidResult};

pub trait Backend {
    type Error: Debug + Display;
    type DeviceId: Debug;
    type Reader: AsyncHidRead;
    type Writer: AsyncHidWrite;

    fn enumerate() -> impl Future<Output = HidResult<impl Stream<Item = DeviceInfo> + Unpin + Send>> + Send;

    fn open(id: &Self::DeviceId, read: bool, write: bool) -> impl Future<Output = HidResult<(Option<Self::Reader>, Option<Self::Writer>)>> + Send;

}

pub type SelectedBackend = win32::Win32Backend;

pub type BackendDeviceId = <SelectedBackend as Backend>::DeviceId;
pub type BackendError = <SelectedBackend as Backend>::Error;
pub type BackendReader = <SelectedBackend as Backend>::Reader;
pub type BackendWriter = <SelectedBackend as Backend>::Writer;
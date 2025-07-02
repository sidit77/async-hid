use std::future::Future;

use futures_lite::stream::Boxed;

use crate::device_info::DeviceId;
use crate::traits::{AsyncHidRead, AsyncHidWrite, HidOperations};
use crate::{DeviceEvent, DeviceInfo, HidResult};

pub type DeviceInfoStream = Boxed<HidResult<DeviceInfo>>;

pub trait Backend: Sized + Default {
    type Reader: AsyncHidRead + HidOperations + Send + Sync;
    type Writer: AsyncHidWrite + Send + Sync;

    fn enumerate(&self) -> impl Future<Output = HidResult<DeviceInfoStream>> + Send;
    fn watch(&self) -> HidResult<Boxed<DeviceEvent>>;

    fn query_info(&self, id: &DeviceId) -> impl Future<Output = HidResult<Vec<DeviceInfo>>> + Send;

    #[allow(clippy::type_complexity)]
    fn open(&self, id: &DeviceId, read: bool, write: bool) -> impl Future<Output = HidResult<(Option<Self::Reader>, Option<Self::Writer>)>> + Send;
}

#[cfg(target_os = "linux")]
mod hidraw;
#[cfg(target_os = "macos")]
mod iohidmanager;
#[cfg(all(target_os = "windows", feature = "win32", not(feature = "winrt")))]
mod win32;
#[cfg(all(target_os = "windows", feature = "winrt"))]
mod winrt;

#[cfg(all(target_os = "windows", feature = "win32", not(feature = "winrt")))]
pub type BackendImpl = win32::Win32Backend;

#[cfg(all(target_os = "windows", feature = "winrt"))]
pub type BackendImpl = winrt::WinRtBackend;

#[cfg(target_os = "linux")]
pub type BackendImpl = hidraw::HidRawBackend;

#[cfg(target_os = "macos")]
pub type BackendImpl = iohidmanager::IoHidManagerBackend;
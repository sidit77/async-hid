use std::hash::Hash;
use std::ops::Deref;
use std::sync::Arc;

use futures_lite::{Stream, StreamExt};
use static_assertions::assert_impl_all;

use crate::backend::{Backend, BackendType, DynBackend};
use crate::{DeviceReader, DeviceReaderWriter, DeviceWriter, HidResult};

/// A platform-specific identifier for a device.
///
/// Can be used as opaque type for equality checks or inspected with platform specific code:
/// ```no_run
/// # use async_hid::DeviceId;
/// let id: DeviceId = /* ... */
/// # panic!();
/// match(id) {
///    #[cfg(target_os = "windows")]
///     DeviceId::UncPath(path) => { /* .. */ },
///     #[cfg(target_os = "linux")]
///     DeviceId::DevPath(path) => { /* .. */ },
///     #[cfg(target_os = "macos")]
///     DeviceId::RegistryEntryId(id) => { /* .. */ }
///     _ => {}
/// }
/// ```
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum DeviceId {
    #[cfg(target_os = "windows")]
    UncPath(windows::core::HSTRING),
    #[cfg(target_os = "linux")]
    DevPath(std::path::PathBuf),
    #[cfg(target_os = "macos")]
    RegistryEntryId(u64)
}
assert_impl_all!(DeviceId: Send, Sync, Unpin);

/// A struct containing basic information about a device
///
/// This struct is part of [Device].
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DeviceInfo {
    /// OS specific identifier
    pub id: DeviceId,
    /// The human-readable name
    pub name: String,
    /// The HID product id assigned to this device
    pub product_id: u16,
    /// The HID vendor id of the device's manufacturer (i.e Logitech = 0x46D)
    pub vendor_id: u16,
    /// The HID usage id
    pub usage_id: u16,
    /// The HID usage page
    pub usage_page: u16,
    /// The serial number of the device. Might be `None` if the device does not have a serial number or the platform/backend does not support retrieving the serial number.
    pub serial_number: Option<String>
}
assert_impl_all!(DeviceInfo: Send, Sync, Unpin);

impl DeviceInfo {
    /// Convenience method for easily finding a specific device
    pub fn matches(&self, usage_page: u16, usage_id: u16, vendor_id: u16, product_id: u16) -> bool {
        self.usage_page == usage_page && self.usage_id == usage_id && self.vendor_id == vendor_id && self.product_id == product_id
    }
}

/// The main entry point of this library
#[derive(Default, Clone)]
pub struct HidBackend(Arc<DynBackend>);

impl HidBackend {
    /// Create a specific backend.
    /// If you don't care and want to just use the default backend for each platform consider calling [HidBackend::default] instead
    pub fn new(backend: BackendType) -> Self {
        Self(Arc::new(DynBackend::new(backend)))
    }

    /// Enumerates all **accessible** HID devices
    ///
    /// If this library fails to retrieve the [DeviceInfo] of a device it will be automatically excluded.
    pub async fn enumerate(&self) -> HidResult<impl Stream<Item = Device> + Send + Unpin + '_> {
        let steam = self.0.enumerate().await?.filter_map(|result| match result {
            Ok(info) => Some(Device {
                backend: self.0.clone(),
                device_info: info
            }),
            Err(_) => None
        });
        Ok(steam)
    }
}

/// A HID device that was detected by calling [HidBackend::enumerate]
pub struct Device {
    backend: Arc<DynBackend>,
    device_info: DeviceInfo
}

impl Deref for Device {
    type Target = DeviceInfo;

    fn deref(&self) -> &Self::Target {
        &self.device_info
    }
}

impl Device {
    /// Open the device in read-only mode
    pub async fn open_readable(&self) -> HidResult<DeviceReader> {
        let (r, _) = self.backend.open(&self.id, true, false).await?;
        Ok(DeviceReader(r.unwrap()))
    }

    /// Open the device in write-only mode
    /// Note: Not all backends support this mode and might upgrade the permission to read+write behind the scenes
    pub async fn open_writeable(&self) -> HidResult<DeviceWriter> {
        let (_, w) = self.backend.open(&self.id, false, true).await?;
        Ok(DeviceWriter(w.unwrap()))
    }

    /// Open the device in read and write mode
    pub async fn open(&self) -> HidResult<DeviceReaderWriter> {
        let (r, w) = self.backend.open(&self.id, true, true).await?;
        Ok((DeviceReader(r.unwrap()), DeviceWriter(w.unwrap())))
    }
}

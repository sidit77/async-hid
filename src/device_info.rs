use crate::backend::{Backend, BackendType, DynBackend};
use crate::{DeviceReader, DeviceReaderWriter, DeviceWriter, HidResult};
use futures_lite::Stream;
use futures_lite::StreamExt;
use static_assertions::assert_impl_all;
use std::hash::Hash;
use std::ops::Deref;
use std::sync::Arc;

#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum DeviceId {
    #[cfg(target_os = "windows")]
    UncPath(windows::core::HSTRING),
    #[cfg(target_os = "linux")]
    DevPath(std::path::PathBuf),
}
assert_impl_all!(DeviceId: Send, Sync, Unpin);

/// A struct containing basic information about a device
///
/// This struct can be obtained by calling [DeviceInfo::enumerate] and upgraded into a usable [Device] by calling [DeviceInfo::open].
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DeviceInfo {
    /// OS specific identifier
    pub id: DeviceId,
    /// The human readable name
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
    pub serial_number: Option<String>,
}
assert_impl_all!(DeviceInfo: Send, Sync, Unpin);

impl DeviceInfo {
    /// Enumerates all **accessible** HID devices
    ///
    /// If this library fails to retrieve the [DeviceInfo] of a device it will be automatically excluded.
    /// Register a `log` compatible logger at `trace` level for more information about the discarded devices.
    //pub fn enumerate() -> impl Future<Output = HidResult<impl Stream<Item = DeviceInfo<B>> + Unpin + Send>> {
    //    B::enumerate()
    //}

    /// Convenience method for easily finding a specific device
    pub fn matches(&self, usage_page: u16, usage_id: u16, vendor_id: u16, product_id: u16) -> bool {
        self.usage_page == usage_page && self.usage_id == usage_id && self.vendor_id == vendor_id && self.product_id == product_id
    }
}

#[derive(Default, Clone)]
pub struct HidBackend(Arc<DynBackend>);

impl HidBackend {
    pub fn new(backend: BackendType) -> Self {
        Self(Arc::new(DynBackend::new(backend)))
    }
    
    pub async fn enumerate(&self) -> HidResult<impl Stream<Item = Device> + Send + Unpin + '_> {
        let steam = self.0
            .enumerate()
            .await?
            .filter_map(|result| match result {
                Ok(info) => Some(Device { backend: self.0.clone(), device_info: info }),
                Err(_) => None
            });
        Ok(steam)
    }
    
}

pub struct Device {
    backend: Arc<DynBackend>,
    device_info: DeviceInfo,
}

impl Deref for Device {
    type Target = DeviceInfo;

    fn deref(&self) -> &Self::Target {
        &self.device_info
    }
}

impl Device {

    pub async fn open_readable(&self) -> HidResult<DeviceReader> {
        let (r, _) = self.backend.open(&self.id, true, false).await?;
        Ok(DeviceReader(r.unwrap()))
    }

    pub async fn open_writeable(&self) -> HidResult<DeviceWriter> {
        let (_, w) = self.backend.open(&self.id, false, true).await?;
        Ok(DeviceWriter(w.unwrap()))
    }

    pub async fn open(&self) -> HidResult<DeviceReaderWriter> {
        let (r, w) = self.backend.open(&self.id, true, true).await?;
        Ok((DeviceReader(r.unwrap()), DeviceWriter(w.unwrap())))
    }

}

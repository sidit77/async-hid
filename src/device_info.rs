use crate::backend::{Backend, DefaultBackend};
use crate::{HidResult};
use futures_core::Stream;
use std::future::Future;
use std::hash::Hash;

/// A struct containing basic information about a device
///
/// This struct can be obtained by calling [DeviceInfo::enumerate] and upgraded into a usable [Device] by calling [DeviceInfo::open].
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DeviceInfo<B: Backend = DefaultBackend> {
    /// OS specific identifier
    pub id: B::DeviceId,
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

impl<B: Backend> DeviceInfo<B> {
    /// Enumerates all **accessible** HID devices
    ///
    /// If this library fails to retrieve the [DeviceInfo] of a device it will be automatically excluded.
    /// Register a `log` compatible logger at `trace` level for more information about the discarded devices.
    pub fn enumerate() -> impl Future<Output = HidResult<impl Stream<Item = DeviceInfo<B>> + Unpin + Send>> {
        B::enumerate()
    }

    /// Convenience method for easily finding a specific device
    pub fn matches(&self, usage_page: u16, usage_id: u16, vendor_id: u16, product_id: u16) -> bool {
        self.usage_page == usage_page && self.usage_id == usage_id && self.vendor_id == vendor_id && self.product_id == product_id
    }
}

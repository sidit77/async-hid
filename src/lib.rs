#![doc = include_str!("../README.md")]

mod backend;
mod error;

use std::fmt::{Debug, Formatter};

pub use error::{ErrorSource, HidError, HidResult};

use crate::backend::{BackendDevice, BackendDeviceId};

/// A struct containing basic information about a device
///
/// This struct can be obtained by calling [DeviceInfo::enumerate] and upgraded into a usable [Device] by calling [DeviceInfo::open].
#[derive(Debug, Clone, Eq, PartialEq)]
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
    pub usage_page: u16
}

impl DeviceInfo {

    /// Enumerates all **accessible** HID devices
    ///
    /// If this library fails to retrieve the [DeviceInfo] of a device it will be automatically excluded.
    /// Register a `log` compatible logger at `trace` level for more information about the discarded devices.
    pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
        backend::enumerate().await
    }

    /// Opens the associated device in the requested [AccessMode]
    pub async fn open(&self, mode: AccessMode) -> HidResult<Device> {
        let dev = backend::open(&self.id.0, mode).await?;
        Ok(Device {
            inner: dev,
            info: self.clone(),
            mode,
        })
    }

    /// Convenience method for easily finding a specific device
    pub fn matches(&self, usage_page: u16, usage_id: u16, vendor_id: u16, product_id: u16) -> bool {
        self.usage_page == usage_page && self.usage_id == usage_id && self.vendor_id == vendor_id && self.product_id == product_id
    }
}

/// A struct representing an opened device
///
/// Dropping this struct will close the associated device
pub struct Device {
    inner: BackendDevice,
    info: DeviceInfo,
    mode: AccessMode
}

impl Device {

    /// Read a input report from this device
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        debug_assert!(self.mode.readable());
        self.inner.read_input_report(buf).await
    }

    /// Write an output report to this device
    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        debug_assert!(self.mode.writeable());
        self.inner.write_output_report(buf).await
    }

    /// Retrieves the [DeviceInfo] associated with this device
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }
}

/// An opaque struct that wraps the OS specific identifier of a device
#[derive(Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct DeviceId(BackendDeviceId);

impl From<BackendDeviceId> for DeviceId {
    fn from(value: BackendDeviceId) -> Self {
        Self(value)
    }
}

impl Debug for DeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// An enum that controls how a device will be opened
///
/// This mainly influences the flags passed to the underlying OS api,
/// but is also used to avoid initializing read specific data structures for write-only devices.
///
/// In general `Read` means shared access and `Write` or `ReadWrite` means exclusive access
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum AccessMode {
    Read,
    Write,
    #[default]
    ReadWrite
}

impl AccessMode {
    pub fn readable(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }
    pub fn writeable(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }
}
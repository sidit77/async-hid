#![doc = include_str!("../README.md")]

mod backend;
mod error;

use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::hash::{Hash, Hasher};

use futures_core::Stream;

use crate::backend::{BackendDevice, BackendDeviceId, BackendPrivateData};
pub use crate::error::{ErrorSource, HidError, HidResult};

/// A struct containing basic information about a device
///
/// This struct can be obtained by calling [DeviceInfo::enumerate] and upgraded into a usable [Device] by calling [DeviceInfo::open].
#[derive(Debug, Clone)]
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

    pub(crate) private_data: BackendPrivateData,
}

impl Hash for DeviceInfo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.name.hash(state);
        self.product_id.hash(state);
        self.vendor_id.hash(state);
        self.usage_id.hash(state);
        self.usage_page.hash(state);
    }
}

impl PartialEq for DeviceInfo {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.product_id == other.product_id
            && self.vendor_id == other.vendor_id
            && self.usage_id == other.usage_id
            && self.usage_page == other.usage_page
    }
}

impl Eq for DeviceInfo {}

impl DeviceInfo {
    /// Enumerates all **accessible** HID devices
    ///
    /// If this library fails to retrieve the [DeviceInfo] of a device it will be automatically excluded.
    /// Register a `log` compatible logger at `trace` level for more information about the discarded devices.
    pub fn enumerate() -> impl Future<Output = HidResult<impl Stream<Item = DeviceInfo> + Unpin + Send>> {
        backend::enumerate()
    }

    /// Opens the associated device in the requested [AccessMode]
    pub async fn open(&self, mode: AccessMode) -> HidResult<Device> {
        let dev = backend::open(&self.id.0, mode).await?;
        Ok(Device {
            inner: dev,
            info: self.clone(),
            mode
        })
    }

    /// Convenience method for easily finding a specific device
    pub fn matches(&self, usage_page: u16, usage_id: u16, vendor_id: u16, product_id: u16) -> bool {
        self.usage_page == usage_page && self.usage_id == usage_id && self.vendor_id == vendor_id && self.product_id == product_id
    }
}

pub trait SerialNumberExt {
    fn serial_number(&self) -> Option<&str>;
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
    pub fn read_input_report<'a>(&'a self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + 'a {
        debug_assert!(self.mode.readable());
        self.inner.read_input_report(buf)
    }

    /// Write an output report to this device
    pub fn write_output_report<'a>(&'a self, buf: &'a [u8]) -> impl Future<Output = HidResult<()>> + 'a {
        debug_assert!(self.mode.writeable());
        self.inner.write_output_report(buf)
    }

    /// Retrieves the [DeviceInfo] associated with this device
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }
}

/// An opaque struct that wraps the OS specific identifier of a device
#[derive(Hash, Clone, Eq, PartialEq)]
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

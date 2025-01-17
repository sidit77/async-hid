#![doc = include_str!("../README.md")]

mod backend;
mod error;

use std::fmt::Debug;
use std::future::Future;
use std::hash::{Hash, Hasher};

use futures_core::Stream;
use static_assertions::assert_impl_all;

pub use crate::backend::BackendError;
pub use crate::error::{ErrorSource, HidError, HidResult};

/// A struct containing basic information about a device.
///
/// This struct can be obtained by calling [DeviceInfo::enumerate] or [DeviceInfo::enumerate_with_criteria].
///
/// A usable [DeviceReader] can be obtained by calling [DeviceInfo::open_readonly] or by calling [DeviceInfo::open] to obtain it in combination with a [DeviceWriter].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceInfo {
    pub(crate) name: String,
    pub(crate) product_id: u16,
    pub(crate) vendor_id: u16,
    pub(crate) usage_id: u16,
    pub(crate) usage_page: u16,

    #[cfg(any(all(target_os = "windows", feature = "win32"), target_os = "macos", target_os = "linux"))]
    pub(crate) serial_number: Option<String>,

    #[cfg(target_os = "windows")]
    pub(crate) handle: windows::core::HSTRING,

    #[cfg(target_os = "macos")]
    pub(crate) registry_entry_id: u64,

    #[cfg(target_os = "linux")]
    pub(crate) path: std::path::PathBuf,
}

impl DeviceInfo {
    /// Enumerates all **accessible** HID devices.
    ///
    /// If this library fails to retrieve the [DeviceInfo] of a device it will be automatically excluded.
    /// Register a `log` compatible logger at `trace` level for more information about the discarded devices.
    pub fn enumerate() -> impl Future<Output = HidResult<impl Stream<Item = DeviceInfo> + Unpin + Send>> {
        backend::enumerate()
    }

    /// Opens the associated device in readonly mode.
    pub async fn open_readonly(&self) -> HidResult<DeviceReader> {
        backend::open_readonly(self).await
    }

    /// Opens the associated device in read/write mode.
    pub async fn open(&self) -> HidResult<(DeviceReader, DeviceWriter)> {
        backend::open(self).await
    }

    /// The human-readable name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The HID vendor id of the device's manufacturer (i.e Logitech = 0x46D).
    pub fn vendor_id(&self) -> u16 {
        self.vendor_id
    }

    /// The HID product id assigned to this device.
    pub fn product_id(&self) -> u16 {
        self.product_id
    }

    /// The HID usage page.
    pub fn usage_page(&self) -> u16 {
        self.usage_page
    }

    /// The HID usage id.
    pub fn usage_id(&self) -> u16 {
        self.usage_id
    }

    #[cfg(any(all(target_os = "windows", feature = "win32"), target_os = "macos", target_os = "linux"))]
    /// *(Windows Win32, macOS & Linux only)* The HID serial number.
    ///
    /// Only available on some USB devices.
    pub fn serial_number(&self) -> Option<&str> {
        self.serial_number.map(|x| x.as_ref())
    }

    #[cfg(target_os = "windows")]
    /// *(Windows only)* Handle identifier for device.
    pub fn handle(&self) -> &windows::core::HSTRING {
        &self.handle
    }

    #[cfg(target_os = "macos")]
    /// *(macOS only)* Registry entry identifier for device.
    pub fn registry_entry_id(&self) -> u64 {
        self.registry_entry_id
    }

    #[cfg(target_os = "linux")]
    /// *(Linux only)* File path to device.
    pub fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    /// Convenience method for easily finding a specific device
    pub fn matches(&self, usage_page: u16, usage_id: u16, vendor_id: u16, product_id: u16) -> bool {
        self.usage_page == usage_page && self.usage_id == usage_id && self.vendor_id == vendor_id && self.product_id == product_id
    }
}

/// A struct representing an opened device reader.
///
/// Dropping this struct and optional associated writer will close the HID.
#[derive(Debug)]
pub struct DeviceReader {
    pub(crate) inner: backend::BackendDeviceReader,
    pub(crate) device_info: DeviceInfo,
}

impl DeviceReader {
    /// Read an input report from this device.
    pub fn read_input_report<'a>(&'a mut self, buffer: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a {
        self.inner.read_input_report(buffer)
    }

    /// Retrieves the [DeviceInfo] associated with this device.
    pub fn device_info(&self) -> &DeviceInfo {
        &self.device_info
    }
}

impl PartialEq for DeviceReader {
    fn eq(&self, other: &Self) -> bool {
        self.device_info.eq(&other.device_info)
    }
}

impl Eq for DeviceReader {}

impl Hash for DeviceReader {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "BackendDeviceReader".hash(state);
        self.device_info.hash(state);
    }
}

/// A struct representing an opened device writer.
///
/// Dropping this struct and associated reader will close the HID.
#[derive(Debug)]
pub struct DeviceWriter {
    pub(crate) inner: backend::BackendDeviceWriter,
    pub(crate) device_info: DeviceInfo,
}

impl DeviceWriter {
    /// Write an output report to this device.
    pub fn write_output_report<'a>(&'a mut self, buffer: &'a [u8]) -> impl Future<Output = HidResult<()>> + Send + 'a {
        self.inner.write_output_report(buffer)
    }

    /// Retrieves the [DeviceInfo] associated with this device.
    pub fn device_info(&self) -> &DeviceInfo {
        &self.device_info
    }
}

impl PartialEq for DeviceWriter {
    fn eq(&self, other: &Self) -> bool {
        self.device_info.eq(&other.device_info)
    }
}

impl Eq for DeviceWriter {}

impl Hash for DeviceWriter {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "BackendDeviceWriter".hash(state);
        self.device_info.hash(state);
    }
}

assert_impl_all!(DeviceReader: Send, Sync);
assert_impl_all!(DeviceWriter: Send, Sync);
assert_impl_all!(DeviceInfo: Send, Sync);

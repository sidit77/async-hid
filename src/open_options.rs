use std::fmt::{Debug, Formatter};

use crate::device::DeviceFeatureHandle;
use crate::{Device, DeviceReader, DeviceReaderWriter, DeviceWriter, HidResult};

/// Whether other handles may use a device while this one holds it open.
///
/// Requested through [OpenOptions::access]. An open either provides the requested mode or fails, with
/// [HidError::Unsupported](crate::HidError::Unsupported) if the backend has no such mode and with
/// [HidError::Busy](crate::HidError::Busy) if another handle holds the device. Without a request the
/// platform default applies: shared on macOS, Win32 and Linux, exclusive on FreeBSD.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Access {
    /// Other handles may open and use the device at the same time, and every one of them receives every
    /// input report. Not available on FreeBSD, where hidraw allows only one open per node.
    Shared,
    /// No other handle can open the device while this one holds it.
    ///
    /// - macOS: the device is seized (`kIOHIDOptionsTypeSeizeDevice`); output and feature report calls of
    ///   handles opened before fail with `Busy` until the seize is released.
    /// - Windows (Win32): the device is opened with share mode `0`; other opens fail with `Busy`, and other
    ///   processes no longer find the device when they enumerate. Devices that Windows keeps open itself,
    ///   such as game controllers, can not be opened exclusively.
    /// - FreeBSD: hidraw allows only one open per node anyway, so while the device is open, enumeration
    ///   skips it and [Device::read_feature_report] fails with `Busy`.
    /// - Linux, WinRT: not available.
    Exclusive
}

/// Options for opening a [Device], obtained from [Device::open_options].
///
/// ```no_run
/// # async fn example(device: async_hid::Device) -> async_hid::HidResult<()> {
/// use async_hid::{Access, HidError};
///
/// match device.open_options().access(Access::Exclusive).open().await {
///     Ok(_handle) => {}
///     Err(HidError::Busy) => println!("The device is held by another handle"),
///     Err(err) => return Err(err)
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
#[must_use = "OpenOptions does nothing until one of its open methods is called"]
pub struct OpenOptions<'a> {
    device: &'a Device,
    access: Option<Access>
}

impl Debug for OpenOptions<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenOptions")
            .field("device", &self.device.id)
            .field("access", &self.access)
            .finish()
    }
}

impl<'a> OpenOptions<'a> {
    pub(crate) fn new(device: &'a Device) -> Self {
        Self { device, access: None }
    }

    /// Request the given [Access] mode instead of the platform default.
    pub fn access(&mut self, access: Access) -> &mut Self {
        self.access = Some(access);
        self
    }

    /// Open the device in read-only mode
    pub async fn open_readable(&self) -> HidResult<DeviceReader> {
        self.device.open_readable_with(self.access).await
    }

    /// Open the device in write-only mode
    /// Note: Not all backends support this mode and might upgrade the permission to read+write behind the scenes
    pub async fn open_writeable(&self) -> HidResult<DeviceWriter> {
        self.device.open_writeable_with(self.access).await
    }

    /// Open the device in read and write mode
    pub async fn open(&self) -> HidResult<DeviceReaderWriter> {
        self.device.open_with(self.access).await
    }

    /// Open a handle for reading and writing feature reports
    pub async fn open_feature_handle(&self) -> HidResult<DeviceFeatureHandle> {
        self.device.open_feature_handle_with(self.access).await
    }
}

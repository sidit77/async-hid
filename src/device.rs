use std::future::Future;
use crate::{AccessMode, DeviceInfo, HidResult};
use crate::backend::BackendDevice;

/// A struct representing an opened device
///
/// Dropping this struct will close the associated device
pub struct Device {
    pub(crate) inner: BackendDevice,
    pub(crate) info: DeviceInfo,
    pub(crate) mode: AccessMode
}

impl Device {
    /// Read a input report from this device
    pub fn read_input_report<'a>(&'a self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a {
        debug_assert!(self.mode.readable());
        self.inner.read_input_report(buf)
    }

    /// Write an output report to this device
    pub fn write_output_report<'a>(&'a self, buf: &'a [u8]) -> impl Future<Output = HidResult<()>> + Send + 'a {
        debug_assert!(self.mode.writeable());
        self.inner.write_output_report(buf)
    }

    /// Retrieves the [DeviceInfo] associated with this device
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }
}

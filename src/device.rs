use crate::backend::{Backend, BackendReader, BackendWriter, SelectedBackend};
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::{DeviceInfo, HidResult};
use std::future::Future;

pub type DeviceReader = BackendReader;
pub type DeviceWriter = BackendWriter;

pub type Device = (DeviceReader, DeviceWriter);

impl AsyncHidRead for Device {
    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output=HidResult<usize>> + Send + 'a {
        self.0.read_input_report(buf)
    }
}

impl AsyncHidWrite for Device {
    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output=HidResult<()>> + Send + 'a {
        self.1.write_output_report(buf)
    }

}

impl DeviceInfo {

    pub async fn open_readable(&self) -> HidResult<DeviceReader> {
        let (r, _) = SelectedBackend::open(&self.id.0, true, false).await?;
        Ok(r.unwrap())
    }

    pub async fn open_writeable(&self) -> HidResult<DeviceWriter> {
        let (_, w) = SelectedBackend::open(&self.id.0, false, true).await?;
        Ok(w.unwrap())
    }

    pub async fn open(&self) -> HidResult<Device> {
        let (r, w) = SelectedBackend::open(&self.id.0, true, true).await?;
        Ok((r.unwrap(), w.unwrap()))
    }

}

/*
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
 */
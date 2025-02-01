use crate::backend::{Backend, DefaultBackend};
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::{DeviceInfo, HidResult};
use std::future::Future;


#[repr(transparent)]
pub struct DeviceReader<B: Backend = DefaultBackend>(B::Reader);

#[repr(transparent)]
pub struct DeviceWriter<B: Backend = DefaultBackend>(B::Writer);

pub type Device<B: Backend = DefaultBackend> = (DeviceReader<B>, DeviceWriter<B>);


impl<B: Backend> AsyncHidRead for DeviceReader<B> {
    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output=HidResult<usize>> + Send + 'a {
        self.0.read_input_report(buf)
    }

}

impl<B: Backend> AsyncHidWrite for DeviceWriter<B> {
    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output=HidResult<()>> + Send + 'a {
        self.0.write_output_report(buf)
    }

}

impl<B: Backend> AsyncHidRead for Device<B> {
    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output=HidResult<usize>> + Send + 'a {
        self.0.read_input_report(buf)
    }
}

impl<B: Backend> AsyncHidWrite for Device<B> {
    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output=HidResult<()>> + Send + 'a {
        self.1.write_output_report(buf)
    }

}

impl<B: Backend> DeviceInfo<B> {

    pub async fn open_readable(&self) -> HidResult<DeviceReader<B>> {
        let (r, _) = B::open(&self.id, true, false).await?;
        Ok(DeviceReader(r.unwrap()))
    }

    pub async fn open_writeable(&self) -> HidResult<DeviceWriter<B>> {
        let (_, w) = B::open(&self.id, false, true).await?;
        Ok(DeviceWriter(w.unwrap()))
    }

    pub async fn open(&self) -> HidResult<Device<B>> {
        let (r, w) = B::open(&self.id, true, true).await?;
        Ok((DeviceReader(r.unwrap()), DeviceWriter(w.unwrap())))
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
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::{Backend, DeviceInfo, HidResult};
use std::future::Future;
use crate::backend::DynBackend;

#[repr(transparent)]
pub struct DeviceReader(<DynBackend as Backend>::Reader);

#[repr(transparent)]
pub struct DeviceWriter(<DynBackend as Backend>::Writer);

pub type Device = (DeviceReader, DeviceWriter);


impl AsyncHidRead for DeviceReader {
    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output=HidResult<usize>> + Send + 'a {
        self.0.read_input_report(buf)
    }

}

impl AsyncHidWrite for DeviceWriter {
    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output=HidResult<()>> + Send + 'a {
        self.0.write_output_report(buf)
    }

}

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
        let (r, _) = B::open(&self.id, true, false).await?;
        Ok(DeviceReader(r.unwrap()))
    }

    pub async fn open_writeable(&self) -> HidResult<DeviceWriter> {
        let (_, w) = B::open(&self.id, false, true).await?;
        Ok(DeviceWriter(w.unwrap()))
    }

    pub async fn open(&self) -> HidResult<Device> {
        let (r, w) = B::open(&self.id, true, true).await?;
        Ok((DeviceReader(r.unwrap()), DeviceWriter(w.unwrap())))
    }

}

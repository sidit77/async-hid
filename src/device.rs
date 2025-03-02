use crate::backend::{Backend, DefaultBackend};
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::{DeviceInfo, HidResult};
use std::future::Future;


#[repr(transparent)]
pub struct DeviceReader<B: Backend = DefaultBackend>(B::Reader);

#[repr(transparent)]
pub struct DeviceWriter<B: Backend = DefaultBackend>(B::Writer);

pub type Device<B = DefaultBackend> = (DeviceReader<B>, DeviceWriter<B>);


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

    pub async fn open_readable(&self) -> HidResult<DeviceReader<B>, B> {
        let (r, _) = B::open(&self.id, true, false).await?;
        Ok(DeviceReader(r.unwrap()))
    }

    pub async fn open_writeable(&self) -> HidResult<DeviceWriter<B>, B> {
        let (_, w) = B::open(&self.id, false, true).await?;
        Ok(DeviceWriter(w.unwrap()))
    }

    pub async fn open(&self) -> HidResult<Device<B>, B> {
        let (r, w) = B::open(&self.id, true, true).await?;
        Ok((DeviceReader(r.unwrap()), DeviceWriter(w.unwrap())))
    }

}

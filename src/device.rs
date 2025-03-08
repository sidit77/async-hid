use crate::backend::DynBackend;
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::{Backend, HidResult};
use std::future::Future;

#[repr(transparent)]
pub struct DeviceReader(pub(crate) <DynBackend as Backend>::Reader);

#[repr(transparent)]
pub struct DeviceWriter(pub(crate) <DynBackend as Backend>::Writer);

pub type DeviceReaderWriter = (DeviceReader, DeviceWriter);


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

impl AsyncHidRead for DeviceReaderWriter {
    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output=HidResult<usize>> + Send + 'a {
        self.0.read_input_report(buf)
    }
}

impl AsyncHidWrite for DeviceReaderWriter {
    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output=HidResult<()>> + Send + 'a {
        self.1.write_output_report(buf)
    }

}


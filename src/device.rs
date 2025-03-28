use std::future::Future;

use crate::backend::{Backend, DynBackend};
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::{HidResult, Report};

/// A reader than can be used to read input reports from a HID device using [AsyncHidRead::read_input_report]
#[repr(transparent)]
pub struct DeviceReader(pub(crate) <DynBackend as Backend>::Reader);

/// A writer than can be used to write output reports from a HID device using [AsyncHidWrite::write_output_report]
#[repr(transparent)]
pub struct DeviceWriter(pub(crate) <DynBackend as Backend>::Writer);

/// Combination of [DeviceReader] and [DeviceWriter]
///
/// Can either be destructured or used directly
pub type DeviceReaderWriter = (DeviceReader, DeviceWriter);

impl AsyncHidRead for DeviceReader {
    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a {
        self.0.read_input_report(buf)
    }
}

impl AsyncHidWrite for DeviceWriter {
    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a mut Report) -> impl Future<Output = HidResult<()>> + Send + 'a {
        self.0.write_output_report(buf)
    }
}

impl AsyncHidRead for DeviceReaderWriter {
    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a {
        self.0.read_input_report(buf)
    }
}

impl AsyncHidWrite for DeviceReaderWriter {
    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a mut Report) -> impl Future<Output = HidResult<()>> + Send + 'a {
        self.1.write_output_report(buf)
    }
}

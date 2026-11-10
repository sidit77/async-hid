use std::future::Future;

use crate::backend::{Backend, DynBackend};
use crate::traits::{AsyncHidFeatureHandle, AsyncHidRead, AsyncHidWrite};
use crate::{ensure, HidError, HidResult};

/// A reader than can be used to read input reports from a HID device using [AsyncHidRead::read_input_report]
#[repr(transparent)]
pub struct DeviceReader(pub(crate) <DynBackend as Backend>::Reader);

/// A writer than can be used to write output reports from a HID device using [AsyncHidWrite::write_output_report]
#[repr(transparent)]
pub struct DeviceWriter(pub(crate) <DynBackend as Backend>::Writer);

/// A reader than can be used to read feature reports from a HID device using [AsyncHidRead::read_feature_report]
#[repr(transparent)]
pub struct DeviceFeatureHandle(pub(crate) <DynBackend as Backend>::FeatureHandle);

/// Combination of [DeviceReader] and [DeviceWriter]
///
/// Can either be destructured or used directly
pub type DeviceReaderWriter = (DeviceReader, DeviceWriter);

/// Rejects an empty buffer: even the shortest report needs one byte.
pub(crate) fn ensure_not_empty(buf: &[u8]) -> HidResult<()> {
    ensure!(!buf.is_empty(), HidError::message("Buffer cannot be empty"));
    Ok(())
}

impl AsyncHidRead for DeviceReader {
    async fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
        ensure_not_empty(buf)?;
        self.0.read_input_report(buf).await
    }
}

impl AsyncHidWrite for DeviceWriter {
    async fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        ensure_not_empty(buf)?;
        self.0.write_output_report(buf).await
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
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output = HidResult<()>> + Send + 'a {
        self.1.write_output_report(buf)
    }
}

impl AsyncHidFeatureHandle for DeviceFeatureHandle {
    async fn read_feature_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
        ensure_not_empty(buf)?;
        self.0.read_feature_report(buf).await
    }

    async fn write_feature_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        ensure_not_empty(buf)?;
        self.0.write_feature_report(buf).await
    }
}

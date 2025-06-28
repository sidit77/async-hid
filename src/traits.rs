use std::future::Future;

use crate::HidResult;

/// Provides functionality for reading from HID devices
pub trait AsyncHidRead {
    /// Read an input report from a HID device.
    ///
    /// The submitted buffer must be big enough to contain the entire report or the report will be truncated
    /// If the device uses numbered report the first byte will contain the report id
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a;
}

/// Provides functionality for writing to HID devices
pub trait AsyncHidWrite {
    /// Write an output report to a HID device
    ///
    /// If the submitted report is larger that what the device expects it might be truncated depending on the backend
    /// The first byte must be the report id. If the device does not use numbered report the first by must be set to 0x0
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output = HidResult<()>> + Send + 'a;
}

/// Provides additional operations for HID devices
pub trait HidOperations {
    /// Get the input report from the HID device.
    /// 
    /// Only use to do immediate reads of the input report.
    /// This should not be used to read input reports in a loop.
    /// For that use `read_input_report` from the `AsyncHidRead` trait.
    fn get_input_report(&self) -> HidResult<Vec<u8>>;
    
    /// Get the feature report from the HID device.
    fn get_feature_report(&self) -> HidResult<Vec<u8>>;
}

impl<O: HidOperations, U> HidOperations for (O, U) {
    fn get_input_report(&self) -> HidResult<Vec<u8>> {
        self.0.get_input_report()
    }

    fn get_feature_report(&self) -> HidResult<Vec<u8>> {
        self.0.get_feature_report()
    }
}

impl<R: AsyncHidRead + Send, U: Send> AsyncHidRead for (R, U)
{
    async fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
        self.0.read_input_report(buf).await
    }
}

impl<W: AsyncHidWrite + Send, U: Send> AsyncHidWrite for (U, W)
{
    async fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        self.1.write_output_report(buf).await
    }
}
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

/// Provides functionality for reading and writing feature reports from HID devices
pub trait AsyncHidFeatureHandle {
    /// Read a feature report from a HID device
    ///
    /// The submitted buffer must be big enough to contain the entire report or the report will be truncated
    /// If the device uses numbered report the first byte will contain the report id
    fn read_feature_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a;

    /// Write a feature report to a HID device
    ///
    /// If the submitted report is larger that what the device expects it might be truncated depending on the backend
    /// The first byte must be the report id. If the device does not use numbered report the first by must be set to 0x0
    fn write_feature_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output = HidResult<()>> + Send + 'a;
}

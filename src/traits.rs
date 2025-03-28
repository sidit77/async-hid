use std::future::Future;
use std::num::NonZeroU8;
use crate::{HidResult, Report};

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

    /// Returns a new buffer with the appropriate size
    fn get_empty_report(&self, report_id: Option<NonZeroU8>) -> Report {
        todo!()
    }
    
    /// Write an output report to a HID device
    ///
    /// If the submitted report is larger that what the device expects it might be truncated depending on the backend
    /// After calling this function the content of `report` is undefined as the underlying backend is free to swap `report` with another buffer
    fn write_output_report<'a>(&'a mut self, report: &'a mut Report) -> impl Future<Output = HidResult<()>> + Send + 'a;

    /*
    TODO:
        - Guarantee that a potential replacement buffer has the same size as the buffers returned by `get_empty_report`
        - Introduce flag in Report to indicate that it's content might've changed?
        - Guarantee that the report id of the replacement buffer matches the report id of the input buffer?
     */
}

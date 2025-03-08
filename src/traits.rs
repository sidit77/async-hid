use std::future::Future;

use crate::HidResult;

pub trait AsyncHidRead {
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a;
}

pub trait AsyncHidWrite {
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output = HidResult<()>> + Send + 'a;
}

use std::slice::{from_raw_parts, from_raw_parts_mut};

use windows::core::{ComInterface, Error, Result};
use windows::Storage::Streams::IBuffer;
use windows::Win32::System::WinRT::IBufferByteAccess;

use crate::{HidError, HidResult};

pub trait IBufferExt {
    fn as_slice(&self) -> Result<&[u8]>;
    fn as_mut_slice(&mut self) -> Result<&mut [u8]>;
}

impl IBufferExt for IBuffer {
    fn as_slice(&self) -> Result<&[u8]> {
        let bytes: IBufferByteAccess = self.cast()?;
        Ok(unsafe { from_raw_parts(bytes.Buffer()?, self.Length()? as usize) })
    }

    fn as_mut_slice(&mut self) -> Result<&mut [u8]> {
        let bytes: IBufferByteAccess = self.cast()?;
        Ok(unsafe { from_raw_parts_mut(bytes.Buffer()?, self.Length()? as usize) })
    }
}

pub trait WinResultExt<T> {
    fn on_null_result<F>(self, func: F) -> HidResult<T>
    where
        F: FnOnce() -> HidError;
}

impl<T> WinResultExt<T> for Result<T> {
    #[track_caller]
    fn on_null_result<F>(self, func: F) -> HidResult<T>
    where
        F: FnOnce() -> HidError
    {
        match self {
            Ok(value) => Ok(value),
            Err(Error::OK) => Err(func()),
            Err(err) => Err(HidError::from(err))
        }
    }
}

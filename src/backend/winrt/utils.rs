use std::slice::{from_raw_parts, from_raw_parts_mut};
use windows::core::ComInterface;
use windows::Storage::Streams::IBuffer;
use windows::Win32::System::WinRT::IBufferByteAccess;
use windows::core::Result;

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


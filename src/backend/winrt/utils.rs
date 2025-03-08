use futures_lite::Stream;
use std::pin::Pin;
use std::slice::{from_raw_parts, from_raw_parts_mut};
use std::task::{Context, Poll};
use windows::core::{Interface, Result};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationCollection};
use windows::Storage::Streams::IBuffer;
use windows::Win32::System::WinRT::IBufferByteAccess;

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
    fn extract_null(self) -> Result<Option<T>>;
}

impl<T> WinResultExt<T> for Result<T> {
    fn extract_null(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) if err.code().is_ok() => Ok(None),
            Err(err) => Err(err)
        }
    }
}

pub struct DeviceInformationSteam {
    devices: DeviceInformationCollection,
    index: u32
}

impl From<DeviceInformationCollection> for DeviceInformationSteam {
    fn from(value: DeviceInformationCollection) -> Self {
        Self {
            devices: value,
            index: 0,
        }
    }
}

impl Stream for DeviceInformationSteam {
    type Item = DeviceInformation;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let current = self.index;
        self.index += 1;
        Poll::Ready(self.devices.GetAt(current).ok())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self
            .devices
            .Size()
            .expect("Failed to get the length of the collection") - self.index) as usize;
        (remaining, Some(remaining))
    }
}

use std::mem::size_of;
use windows::core::{PCWSTR};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetSerialNumberString;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};

use crate::{DeviceInfo, SerialNumberExt};
use crate::backend::BackendDeviceId;
use crate::error::HidResult;

impl SerialNumberExt for DeviceInfo {
    fn serial_number(&self) -> Option<&str> {
        self
            .private_data
            .serial_number
            .get_or_init(|| get_serial_number(&self.id.0)
                .map_err(|err| log::trace!("Failed to query additional information:\n\t{:?}", err))
                .ok())
            .as_ref()
            .map(String::as_str)
    }
}

fn get_serial_number(path: &BackendDeviceId) -> HidResult<String> {
    let handle = open_device(PCWSTR::from_raw(path.as_ptr()))?;
    let mut buffer = [0u16; 256];
    unsafe { HidD_GetSerialNumberString(handle.as_raw(), buffer.as_mut_ptr() as _, (size_of::<u16>() * buffer.len()) as u32) }.ok()?;
    let serial_number = buffer
        .split(|c| *c == 0x0)
        .map(String::from_utf16_lossy)
        .next()
        .expect("Failed to interpret string");
    Ok(serial_number)
}

fn open_device(path: PCWSTR) -> HidResult<Handle> {
    let handle = unsafe {
        CreateFileW(
            path,
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            HANDLE::default()
        )
    }?;
    Ok(Handle::from_raw(handle))
}

pub struct Handle(HANDLE);

impl Handle {
    pub fn from_raw(handle: HANDLE) -> Self {
        Self(handle)
    }
    pub fn as_raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if self.0.is_invalid() {
            unsafe {
                CloseHandle(self.0)
                    .unwrap_or_else(|err| log::debug!("Failed to close handle: {}", err))
            }
        }
        self.0 = HANDLE::default();
    }
}

use std::ffi::c_void;

use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetManufacturerString, HidD_GetPreparsedData, HidD_GetProductString, HidD_GetSerialNumberString, HidP_GetCaps,
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA
};
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SHARING_VIOLATION, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE, FILE_SHARE_NONE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING
};

use crate::backend::win32::check_error;
use crate::{ensure, Access, HidError, HidResult};

#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Device(HANDLE);

unsafe impl Send for Device {}
unsafe impl Sync for Device {}

/// The `dwShareMode` of `CreateFileW` for the requested access.
///
/// Without a request the device is opened the way it always was: shared for
/// reading and writing. An exclusive open shares nothing.
pub fn share_mode(access: Option<Access>) -> FILE_SHARE_MODE {
    match access {
        None | Some(Access::Shared) => FILE_SHARE_READ | FILE_SHARE_WRITE,
        Some(Access::Exclusive) => FILE_SHARE_NONE
    }
}

/// The error for a failed `CreateFileW` on a device path.
pub fn open_error(error: windows::core::Error) -> HidError {
    match error.code() {
        code if code == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0) => HidError::NotConnected,
        code if code == HRESULT::from_win32(ERROR_SHARING_VIOLATION.0) => HidError::Busy,
        _ => error.into()
    }
}

impl Device {
    pub fn open(path: PCWSTR, read: bool, write: bool, access: Option<Access>) -> HidResult<Device> {
        let handle = unsafe {
            CreateFileW(
                path,
                // The desired access. The FILE_SHARE_* constants have the
                // values of FILE_READ_DATA and FILE_WRITE_DATA. The share
                // mode is the argument after this one.
                match (read, write) {
                    (true, false) => FILE_SHARE_READ,
                    (false, true) => FILE_SHARE_WRITE,
                    (true, true) => FILE_SHARE_READ | FILE_SHARE_WRITE,
                    (false, false) => FILE_SHARE_NONE
                }
                .0,
                share_mode(access),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None
            )
        };
        handle.map(Device).map_err(open_error)
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }

    pub fn attributes(&self) -> HidResult<HIDD_ATTRIBUTES> {
        let mut attributes = HIDD_ATTRIBUTES::default();
        check_error(unsafe { HidD_GetAttributes(self.0, &mut attributes) })?;
        Ok(attributes)
    }

    pub fn preparsed_data(&self) -> HidResult<PreparsedData> {
        PreparsedData::from_device(self)
    }

    #[track_caller]
    fn read_string(&self, func: unsafe fn(HANDLE, *mut c_void, u32) -> bool) -> Option<String> {
        let mut buffer = [0u16; 512];
        ensure!(unsafe { func(self.0, buffer.as_mut_ptr() as _, (size_of::<u16>() * buffer.len()) as u32) });

        let serial_number = buffer
            .split(|c| *c == 0x0)
            .map(String::from_utf16_lossy)
            .next()
            .expect("Failed to interpret string");
        Some(serial_number)
    }

    pub fn serial_number(&self) -> Option<String> {
        //Silently discard errors
        self.read_string(HidD_GetSerialNumberString)
    }

    pub fn name(&self) -> HidResult<String> {
        self.read_string(HidD_GetProductString)
            .ok_or_else(|| windows::core::Error::from_thread().into())
    }

    pub fn manufacturer(&self) -> Option<String> {
        self.read_string(HidD_GetManufacturerString)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0).unwrap_or_else(|err| log::warn!("Failed to close device handle: {}", err)) }
    }
}

#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct PreparsedData(PHIDP_PREPARSED_DATA);

impl PreparsedData {
    pub fn from_device(device: &Device) -> HidResult<PreparsedData> {
        let mut preparsed_data = PHIDP_PREPARSED_DATA::default();
        check_error(unsafe { HidD_GetPreparsedData(device.0, &mut preparsed_data) })?;
        Ok(PreparsedData(preparsed_data))
    }

    pub fn caps(&self) -> HidResult<HIDP_CAPS> {
        let mut caps = HIDP_CAPS::default();
        check_error(unsafe { HidP_GetCaps(self.0, &mut caps) } == HIDP_STATUS_SUCCESS)?;
        Ok(caps)
    }
}

impl Drop for PreparsedData {
    fn drop(&mut self) {
        check_error(unsafe { HidD_FreePreparsedData(self.0) }).unwrap_or_else(|err| log::warn!("Failed to free preparsed data: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SHARING_VIOLATION};
    use windows::Win32::Storage::FileSystem::{FILE_SHARE_NONE, FILE_SHARE_READ, FILE_SHARE_WRITE};

    use super::{open_error, share_mode};
    use crate::{Access, HidError};

    fn win32_error(code: u32) -> windows::core::Error {
        windows::core::Error::from_hresult(HRESULT::from_win32(code))
    }

    #[test]
    fn the_default_and_shared_access_share_reading_and_writing() {
        assert_eq!(share_mode(None), FILE_SHARE_READ | FILE_SHARE_WRITE);
        assert_eq!(share_mode(Some(Access::Shared)), FILE_SHARE_READ | FILE_SHARE_WRITE);
    }

    #[test]
    fn exclusive_access_shares_nothing() {
        assert_eq!(share_mode(Some(Access::Exclusive)), FILE_SHARE_NONE);
    }

    #[test]
    fn sharing_violation_is_busy() {
        assert!(matches!(open_error(win32_error(ERROR_SHARING_VIOLATION.0)), HidError::Busy));
    }

    #[test]
    fn file_not_found_is_not_connected() {
        assert!(matches!(open_error(win32_error(ERROR_FILE_NOT_FOUND.0)), HidError::NotConnected));
    }

    #[test]
    fn other_errors_are_passed_on() {
        assert!(matches!(open_error(win32_error(ERROR_ACCESS_DENIED.0)), HidError::Other(_)));
    }
}

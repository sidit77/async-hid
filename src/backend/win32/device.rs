use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Devices::HumanInterfaceDevice::{HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetPreparsedData, HidD_GetProductString, HidD_GetSerialNumberString, HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS, PHIDP_PREPARSED_DATA};
use windows::Win32::Foundation::{CloseHandle, BOOLEAN, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use crate::{AccessMode, HidResult};

#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Device(HANDLE);

impl Device {

    pub fn open(path: PCWSTR, access_mode: Option<AccessMode>) -> HidResult<Device> {
        let handle = unsafe {
            CreateFileW(
                path,
                match access_mode {
                    Some(AccessMode::Read) => FILE_SHARE_READ,
                    Some(AccessMode::Write) => FILE_SHARE_WRITE,
                    Some(AccessMode::ReadWrite) => FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None => FILE_SHARE_NONE,
                }.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                HANDLE::default()
            )
        }?;
        Ok(Device(handle))
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }

    pub fn attributes(&self) -> HidResult<HIDD_ATTRIBUTES> {
        let mut attributes = HIDD_ATTRIBUTES::default();
        unsafe {
            HidD_GetAttributes(self.0, &mut attributes).ok()?;
        }
        Ok(attributes)
    }

    pub fn preparsed_data(&self) -> HidResult<PreparsedData> {
        PreparsedData::from_device(self)
    }

    fn read_string(&self, func: unsafe fn(HANDLE, *mut c_void, u32) -> BOOLEAN) -> HidResult<String> {
        let mut buffer = [0u16; 256];
        unsafe { func(self.0, buffer.as_mut_ptr() as _, (size_of::<u16>() * buffer.len()) as u32) }.ok()?;
        let serial_number = buffer
            .split(|c| *c == 0x0)
            .map(String::from_utf16_lossy)
            .next()
            .expect("Failed to interpret string");
        Ok(serial_number)
    }

    pub fn serial_number(&self) -> HidResult<String> {
        self.read_string(HidD_GetSerialNumberString)
    }

    pub fn name(&self) -> HidResult<String> {
        self.read_string(HidD_GetProductString)
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
        unsafe {
            HidD_GetPreparsedData(device.0, &mut preparsed_data).ok()?;
        }
        Ok(PreparsedData(preparsed_data))
    }

    pub fn caps(&self) -> HidResult<HIDP_CAPS> {
        let mut caps = HIDP_CAPS::default();
        unsafe {
            HidP_GetCaps(self.0, &mut caps).ok()?;
        }
        Ok(caps)
    }

}

impl Drop for PreparsedData {
    fn drop(&mut self) {
        unsafe {
            HidD_FreePreparsedData(self.0)
                .ok()
                .unwrap_or_else(|err| log::warn!("Failed to free preparsed data: {}", err))
        }
    }
}
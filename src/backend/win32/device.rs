use std::ffi::c_void;

use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature, HidD_GetInputReport, HidD_GetPreparsedData, HidD_GetProductString, HidD_GetSerialNumberString, HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA
};
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};

use crate::backend::win32::check_error;
use crate::{ensure, HidError, HidResult};

#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Device(HANDLE);

unsafe impl Send for Device {}
unsafe impl Sync for Device {}

impl Device {
    pub fn open(path: PCWSTR, read: bool, write: bool) -> HidResult<Device> {
        let handle = unsafe {
            CreateFileW(
                path,
                match (read, write) {
                    (true, false) => FILE_SHARE_READ,
                    (false, true) => FILE_SHARE_WRITE,
                    (true, true) => FILE_SHARE_READ | FILE_SHARE_WRITE,
                    (false, false) => FILE_SHARE_NONE
                }
                .0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None
            )
        };
        handle.map(Device).map_err(|e| match e {
            e if e.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0) => HidError::NotConnected,
            e => e.into()
        })
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
        ensure!(unsafe { func(self.0, buffer.as_mut_ptr() as _, size_of_val(&buffer) as u32) });

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
            .ok_or_else(|| windows::core::Error::from_win32().into())
    }

    pub fn get_input_report(&self, input_report_length: usize) -> HidResult<Vec<u8>> {
        let mut buf: Vec<u8> = vec![0; input_report_length];
        check_error(unsafe { HidD_GetInputReport(self.0, buf.as_mut_ptr() as _, buf.capacity() as u32) })?;
        Ok(buf)
    }

    pub fn get_feature_report(&self, feature_report_length: usize) -> HidResult<Vec<u8>> {
        let mut buf: Vec<u8> = vec![0; feature_report_length];
        check_error(unsafe { HidD_GetFeature(self.0, buf.as_mut_ptr() as _, buf.capacity() as u32) })?;
        Ok(buf)
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
        log::info!("HIDP_CAPS: {:?}", caps);
        Ok(caps)
    }
}

impl Drop for PreparsedData {
    fn drop(&mut self) {
        check_error(unsafe { HidD_FreePreparsedData(self.0) }).unwrap_or_else(|err| log::warn!("Failed to free preparsed data: {}", err))
    }
}

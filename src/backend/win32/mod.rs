
mod device;
mod waiter;
mod buffer;
mod string;
mod interface;
mod mutex;

use std::sync::{Arc};

use futures_lite::Stream;
use futures_lite::stream::iter;
use windows::core::{HRESULT};
use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_MapCrToWin32Err, CONFIGRET};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_SetNumInputBuffers;
use windows::Win32::Foundation::E_FAIL;
use crate::error::{ErrorSource, HidResult};
use crate::{ensure, AccessMode, DeviceId, DeviceInfo, HidError, SerialNumberExt};
use crate::backend::win32::buffer::{IoBuffer, Readable, Writable};
use crate::backend::win32::device::Device;
use interface::Interface;
use crate::backend::win32::mutex::SimpleMutex;
use crate::backend::win32::string::{U16Str, U16String};

pub async fn enumerate() -> HidResult<impl Stream<Item = DeviceInfo> + Unpin + Send> {
    let devices = Interface::get_interface_list()?
        .iter()
        .filter_map(|i| {
            get_device_information(i)
                .map_err(|e| log::trace!("Failed to query device information for {i:?}\n\tbecause {e}"))
                .ok()
        })
        .collect::<Vec<_>>();
    Ok(iter(devices))
}

impl SerialNumberExt for DeviceInfo {
    fn serial_number(&self) -> Option<&str> {
        self.private_data
            .serial_number
            .as_ref()
            .map(String::as_str)
    }
}

fn get_device_information(device: &U16Str) -> HidResult<DeviceInfo> {
    let id = device.to_owned();
    let device = Device::open(device.as_ptr(), None)?;
    let name = device.name()?;
    let attribs = device.attributes()?;
    let caps = device.preparsed_data()?.caps()?;
    let serial_number = device.serial_number().ok();
    Ok(DeviceInfo {
        id: DeviceId::from(id),
        name,
        product_id: attribs.ProductID,
        vendor_id: attribs.VendorID,
        usage_id: caps.Usage,
        usage_page: caps.UsagePage,
        private_data: BackendPrivateData {
            serial_number
        }
    })
}


#[derive(Debug)]
pub struct BackendDevice {
    read_buffer: SimpleMutex<IoBuffer<Readable>>,
    write_buffer: SimpleMutex<IoBuffer<Writable>>,
}

pub async fn open(id: &BackendDeviceId, mode: AccessMode) -> HidResult<BackendDevice> {
    let device = Arc::new(Device::open(id.as_ptr(), Some(mode))?);

    unsafe {
        HidD_SetNumInputBuffers(device.handle(), 64).ok()?;
    }
    let caps = device.preparsed_data()?.caps()?;

    let read_buffer = SimpleMutex::new(IoBuffer::<Readable>::new(device.clone(), caps.InputReportByteLength as usize)?);
    let write_buffer = SimpleMutex::new(IoBuffer::<Writable>::new(device, caps.OutputReportByteLength as usize)?);
    Ok(BackendDevice {
        read_buffer,
        write_buffer,
    })
}

impl BackendDevice {
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        match self.read_buffer.try_lock() {
            Some(mut buffer) => {
                let len = buffer.read(buf).await?;
                Ok(len)
            },
            None => Err(HidError::custom("Another read operation is in progress"))
        }
    }

    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        ensure!(!buf.is_empty(), HidError::zero_sized_data());
        match self.write_buffer.try_lock() {
            Some(mut buffer) => {
                buffer.write(buf).await?;
                Ok(())
            },
            None => Err(HidError::custom("Another write operation is in progress"))
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendPrivateData {
    serial_number: Option<String>
}

pub type BackendDeviceId = U16String;
pub type BackendError = windows::core::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}

impl From<CONFIGRET> for ErrorSource {
    fn from(value: CONFIGRET) -> Self {
        const UNKNOWN_ERROR: u32 = 0xFFFF;
        let hresult = match unsafe { CM_MapCrToWin32Err(value, UNKNOWN_ERROR) } {
            UNKNOWN_ERROR => E_FAIL,
            win32 => HRESULT::from_win32(win32),
        };
        ErrorSource::PlatformSpecific(windows::core::Error::from(hresult))
    }
}
mod buffer;
mod device;
mod interface;
mod string;
mod waiter;

use std::future::Future;
use std::sync::Arc;

use futures_lite::stream::{iter, Boxed};
use futures_lite::StreamExt;
use interface::Interface;
use windows::core::{HRESULT, HSTRING, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_MapCrToWin32Err, CONFIGRET};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_SetNumInputBuffers;
use windows::Win32::Foundation::E_FAIL;

use crate::backend::win32::buffer::{IoBuffer, Readable, Writable};
use crate::backend::win32::device::Device;
use crate::backend::win32::interface::DeviceNotificationStream;
use crate::backend::{Backend, DeviceInfoStream};
use crate::device_info::DeviceId;
use crate::error::HidResult;
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::{DeviceEvent, DeviceInfo, HidError};

#[derive(Default)]
pub struct Win32Backend;

impl Backend for Win32Backend {
    type Reader = IoBuffer<Readable>;
    type Writer = IoBuffer<Writable>;

    async fn enumerate(&self) -> HidResult<DeviceInfoStream> {
        let device_ids = Interface::get_interface_list()?
            .iter()
            .map(HSTRING::from)
            .collect::<Vec<_>>();
        let device_infos = device_ids.into_iter().map(get_device_information);
        Ok(iter(device_infos).boxed())
    }

    fn watch(&self) -> HidResult<Boxed<DeviceEvent>> {
        Ok(DeviceNotificationStream::new()?.boxed())
    }

    async fn query_info(&self, id: &DeviceId) -> HidResult<Vec<DeviceInfo>> {
        Ok(vec![get_device_information(id.clone())?])
    }

    async fn open(&self, id: &DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        let id = match id {
            p => PCWSTR::from_raw(p.as_ptr())
        };
        let device = Arc::new(Device::open(id, read, write)?);

        if read {
            check_error(unsafe { HidD_SetNumInputBuffers(device.handle(), 64) })?;
        }

        let caps = device.preparsed_data()?.caps()?;

        let read_buffer = match read {
            true => Some(IoBuffer::<Readable>::new(device.clone(), caps.InputReportByteLength as usize)?),
            false => None
        };
        let write_buffer = match write {
            true => Some(IoBuffer::<Writable>::new(device.clone(), caps.OutputReportByteLength as usize)?),
            false => None
        };
        Ok((read_buffer, write_buffer))
    }
}

fn get_device_information(id: HSTRING) -> HidResult<DeviceInfo> {
    let device = Device::open(PCWSTR(id.as_ptr()), false, false)?;
    let name = device.name()?;
    let attribs = device.attributes()?;
    let caps = device.preparsed_data()?.caps()?;
    let serial_number = device.serial_number();
    Ok(DeviceInfo {
        id,
        name,
        product_id: attribs.ProductID,
        vendor_id: attribs.VendorID,
        usage_id: caps.Usage,
        usage_page: caps.UsagePage,
        serial_number
    })
}

impl AsyncHidRead for IoBuffer<Readable> {
    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a {
        self.read(buf)
    }
}

impl AsyncHidWrite for IoBuffer<Writable> {
    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output = HidResult<()>> + Send + 'a {
        self.write(buf)
    }
}

pub fn check_error(result: bool) -> windows::core::Result<()> {
    if result {
        Ok(())
    } else {
        Err(windows::core::Error::from_win32())
    }
}

impl From<CONFIGRET> for HidError {
    #[track_caller]
    fn from(value: CONFIGRET) -> Self {
        const UNKNOWN_ERROR: u32 = 0xFFFF;
        let hresult = match unsafe { CM_MapCrToWin32Err(value, UNKNOWN_ERROR) } {
            UNKNOWN_ERROR => E_FAIL,
            win32 => HRESULT::from_win32(win32)
        };
        HidError::from(windows::core::Error::from(hresult))
    }
}

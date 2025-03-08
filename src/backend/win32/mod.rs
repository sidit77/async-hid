
mod device;
mod waiter;
mod buffer;
mod string;
mod interface;

use std::future::Future;
use std::sync::{Arc};

use futures_lite::{StreamExt};
use futures_lite::stream::iter;
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_MapCrToWin32Err, CONFIGRET};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_SetNumInputBuffers;
use windows::Win32::Foundation::E_FAIL;
use crate::error::{HidResult};
use crate::{DeviceInfo, HidError};
use crate::backend::win32::buffer::{IoBuffer, Readable, Writable};
use crate::backend::win32::device::Device;
use interface::Interface;
use crate::backend::{Backend, DeviceInfoStream};
use crate::backend::win32::string::{U16Str};
use crate::device_info::DeviceId;
use crate::traits::{AsyncHidRead, AsyncHidWrite};

fn get_device_information(device: &U16Str) -> HidResult<DeviceInfo> {
    let id = DeviceId::UncPath(device.into());
    let device = Device::open(device.as_ptr(), false, false)?;
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

#[derive(Default)]
pub struct Win32Backend;

impl Backend for Win32Backend {
    //type DeviceId = U16String;
    type Reader = IoBuffer<Readable>;
    type Writer = IoBuffer<Writable>;

    async fn enumerate(&self) -> HidResult<DeviceInfoStream>{
        let devices = Interface::get_interface_list()?
            .iter()
            .filter_map(|i| {
                get_device_information(i)
                    .map_err(|e| log::trace!("Failed to query device information for {i:?}: {e}"))
                    .ok()
            })
            .collect::<Vec<_>>();
        Ok(iter(devices).boxed())
    }

    async fn open(&self, id: &DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        let id = match id {
            DeviceId::UncPath(p) => PCWSTR::from_raw(p.as_ptr())
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
impl AsyncHidRead for IoBuffer<Readable> {

    #[inline]
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output=HidResult<usize>> + Send + 'a {
        self.read(buf)
    }
}

impl AsyncHidWrite for IoBuffer<Writable> {

    #[inline]
    fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> impl Future<Output=HidResult<()>> + Send + 'a {
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
            win32 => HRESULT::from_win32(win32),
        };
        HidError::from(windows::core::Error::from(hresult))
    }
}
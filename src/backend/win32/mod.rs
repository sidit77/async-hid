
mod device;
mod waiter;
mod buffer;

use std::fmt::Display;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures_lite::{Stream, StreamExt};
use windows::core::{h, HSTRING, PCWSTR};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationCollection};
use windows::Storage::FileAccessMode;
use windows::Win32::Devices::HumanInterfaceDevice::HidD_SetNumInputBuffers;
use crate::error::{ErrorSource, HidResult};
use crate::{ensure, AccessMode, DeviceInfo, HidError, SerialNumberExt};
use crate::backend::win32::buffer::{IoBuffer, Readable, Writable};
use crate::backend::win32::device::Device;

const DEVICE_SELECTOR: &HSTRING = h!(
    r#"System.Devices.InterfaceClassGuid:="{4D1E55B2-F16F-11CF-88CB-001111000030}" AND System.Devices.InterfaceEnabled:=System.StructuredQueryType.Boolean#True"#
);

pub async fn enumerate() -> HidResult<impl Stream<Item = DeviceInfo> + Unpin + Send> {
    //let devices = DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
    //    .await?
    //    .into_iter()
    //    .map(get_device_information)
    //    .collect::<FuturesUnordered<_>>()
    //    .filter_map(|info| ready(info.ok()))
    //    .collect()
    //    .await;
    let devices = DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
        .await?;
    let devices = DeviceInformationSteam::from(devices)
        .map(get_device_information)
        .filter_map(|r| {
            r.map_err(|e| log::trace!("Failed to query device information\n\tbecause {e:?}"))
                .ok()
        });
    //.collect()
    //.await;
    Ok(devices)
}

impl SerialNumberExt for DeviceInfo {
    fn serial_number(&self) -> Option<&str> {
        self.private_data
            .serial_number
            .as_ref()
            .map(String::as_str)
    }
}

fn get_device_information(device: DeviceInformation) -> HidResult<DeviceInfo> {
    let id = device.Id()?;
    let name = device.Name()?.to_string_lossy();
    let device = Device::open(PCWSTR(id.as_ptr()), None)?;
    let attribs = device.attributes()?;
    let caps = device.preparsed_data()?.caps()?;
    let serial_number = device.serial_number().ok();
    Ok(DeviceInfo {
        id: HashableHSTRING(id).into(),
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
    read_buffer: Mutex<IoBuffer<Readable>>,
    write_buffer: Mutex<IoBuffer<Writable>>,
}

pub async fn open(id: &BackendDeviceId, mode: AccessMode) -> HidResult<BackendDevice> {
    let device = Arc::new(Device::open(PCWSTR(id.as_ptr()), Some(mode))?);

    unsafe {
        HidD_SetNumInputBuffers(device.handle(), 64).ok()?;
    }
    let caps = device.preparsed_data()?.caps()?;

    let read_buffer = Mutex::new(IoBuffer::<Readable>::new(device.clone(), caps.InputReportByteLength as usize)?);
    let write_buffer = Mutex::new(IoBuffer::<Writable>::new(device, caps.OutputReportByteLength as usize)?);
    Ok(BackendDevice {
        read_buffer,
        write_buffer,
    })
}

impl BackendDevice {
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        match self.read_buffer.try_lock() {
            Ok(mut buffer) => {
                let len = buffer.read(buf).await?;
                Ok(len)
            },
            Err(_) => Err(HidError::custom("Another read operation is in progress"))
        }
    }

    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        ensure!(!buf.is_empty(), HidError::zero_sized_data());
        match self.write_buffer.try_lock() {
            Ok(mut buffer) => {
                buffer.write(buf).await?;
                Ok(())
            },
            Err(_) => Err(HidError::custom("Another write operation is in progress"))
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendPrivateData {
    serial_number: Option<String>
}

/// Wrapper type for HSTRING to add Hash implementation
///
/// windows-rs has a built-in Hash HSTRING implementation after version 0.55.0 (introduced by this PR https://github.com/microsoft/windows-rs/pull/2924/files)
/// Though, a direct upgrade to the newer windows-rs versions would require further work due to API and functionality changes
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HashableHSTRING(HSTRING);

impl Display for HashableHSTRING {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for HashableHSTRING {
    type Target = HSTRING;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HashableHSTRING {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Hash for HashableHSTRING {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.0.as_wide().hash(hasher)
    }
}

pub type BackendDeviceId = HashableHSTRING;
pub type BackendError = windows::core::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}

impl From<AccessMode> for FileAccessMode {
    fn from(value: AccessMode) -> Self {
        match value {
            AccessMode::Read => FileAccessMode::Read,
            AccessMode::Write => FileAccessMode::ReadWrite,
            AccessMode::ReadWrite => FileAccessMode::ReadWrite
        }
    }
}

struct DeviceInformationSteam {
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

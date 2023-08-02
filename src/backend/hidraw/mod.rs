mod descriptor;
mod ioctl;

use std::fs::OpenOptions;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use nix::fcntl::OFlag;
use nix::unistd::{read, write};
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::task::spawn_blocking;

use udev::{Device, Enumerator};

use crate::{DeviceInfo, ensure, ErrorSource, HidError, HidResult};
use crate::backend::hidraw::descriptor::HidrawReportDescriptor;
use crate::backend::hidraw::ioctl::hidraw_ioc_grdescsize;

pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
    spawn_blocking(enumerate_sync)
        .await
        .map_err( |_| HidError::custom("Background task failed"))?
}

fn enumerate_sync() -> HidResult<Vec<DeviceInfo>> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_subsystem("hidraw")?;
    let devices = enumerator
        .scan_devices()?
        .map(get_device_info)
        .filter_map(Result::ok)
        .flatten()
        .collect();
    Ok(devices)
}


fn get_device_info(raw_device: Device) -> HidResult<Vec<DeviceInfo>> {
    let device = raw_device
        .parent_with_subsystem("hid")?
        .ok_or(HidError::custom("Can't find hid interface"))?;

    let (_bus, vendor_id, product_id) = device
        .property_value("HID_ID")
        .and_then(|s| s.to_str())
        .and_then(parse_hid_vid_pid)
        .ok_or(HidError::custom("Can't find hid ids"))?;

    let id = raw_device
        .devnode()
        .ok_or(HidError::custom("Can't find device node"))?
        .to_path_buf();

    let name = device
        .property_value("HID_NAME")
        .ok_or(HidError::custom("Can't find hid name"))?
        .to_string_lossy()
        .to_string();

    let info = DeviceInfo {
        id: id.into(),
        name,
        product_id,
        vendor_id,
        usage_id: 0,
        usage_page: 0
    };
    let results = HidrawReportDescriptor::from_syspath(raw_device.syspath())
        .map(|descriptor| descriptor
            .usages()
            .map(|(usage_page, usage_id)| DeviceInfo {
                usage_page,
                usage_id,
                ..info.clone()
            })
            .collect())
        .unwrap_or_else(|_| vec![info]);
    Ok(results)
}

fn parse_hid_vid_pid(s: &str) -> Option<(u16, u16, u16)> {
    let mut elems = s
        .split(':')
        .filter_map(|s| u16::from_str_radix(s, 16).ok());
    let devtype = elems.next()?;
    let vendor = elems.next()?;
    let product = elems.next()?;

    Some((devtype, vendor, product))
}

#[derive(Debug)]
pub struct BackendDevice {
    fd: AsyncFd<OwnedFd>
}

impl BackendDevice {

    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        self
            .fd
            .async_io(Interest::READABLE, |fd| read(fd.as_raw_fd(), buf)
                .map_err(BackendError::from))
            .await
            .map_err(HidError::from)
    }

    pub async fn write_output_report(&self, data: &[u8]) -> HidResult<()> {
        ensure!(!data.is_empty(), HidError::zero_sized_data());
        self
            .fd
            .async_io(Interest::WRITABLE, |fd| write(fd.as_raw_fd(), data)
                .map_err(BackendError::from))
            .await
            .map_err(HidError::from)
            .map(|i| debug_assert_eq!(i, data.len()))
    }

}

pub async fn open(id: &BackendDeviceId, mode: AccessMode) -> HidResult<BackendDevice> {

    let fd: OwnedFd = OpenOptions::new()
        .read(mode.readable())
        .write(mode.writeable())
        .custom_flags((OFlag::O_CLOEXEC | OFlag::O_NONBLOCK).bits())
        .open(id)?
        .into();

    let mut size = 0i32;
    unsafe { hidraw_ioc_grdescsize(fd.as_raw_fd(), &mut size) }
        .map_err(|e| HidError::custom(format!("ioctl(GRDESCSIZE) error for {:?}, not a HIDRAW device?: {}", id, e)))?;

    Ok(BackendDevice {
        fd: AsyncFd::new(fd)?,
    })
}

pub type BackendDeviceId = PathBuf;
pub type BackendError = std::io::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}


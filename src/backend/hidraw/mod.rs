mod descriptor;
mod ioctl;

use std::fs::OpenOptions;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::poll::{poll, PollFd, PollFlags};
use nix::unistd::read;
use tokio::task::spawn_blocking;

use udev::{Device, Enumerator};

use crate::{DeviceInfo, ErrorSource, HidError, HidResult};
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
    fd: OwnedFd
}

impl BackendDevice {
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        let pollfd = PollFd::new(self.fd.as_raw_fd(), PollFlags::POLLIN);
        let res = poll(&mut [pollfd], -1)
            .map_err(|e| HidError::custom(format!("Errno: {e}")))?;

        if res == 0 {
            return Ok(0);
        }

        let events = pollfd
            .revents()
            .map(|e| e.intersects(PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL));

        if events.is_none() || events == Some(true) {
            return Err(HidError::custom("unexpected poll error (device disconnected)"));
        }

        match read(self.fd.as_raw_fd(), buf) {
            Ok(w) => Ok(w),
            Err(Errno::EAGAIN) | Err(Errno::EINPROGRESS) => Ok(0),
            Err(e) => Err(HidError::custom(format!("Errno: {e}"))),
        }
    }
    pub async fn write_output_report(&self, _buf: &[u8]) -> HidResult<()> { Ok(()) }
}

pub async fn open(id: &BackendDeviceId) -> HidResult<BackendDevice> {

    let fd: OwnedFd = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags((OFlag::O_CLOEXEC | OFlag::O_NONBLOCK).bits())
        .open(id)?
        .into();

    let mut size = 0i32;
    unsafe { hidraw_ioc_grdescsize(fd.as_raw_fd(), &mut size) }
        .map_err(|e| HidError::custom(format!("ioctl(GRDESCSIZE) error for {:?}, not a HIDRAW device?: {}", id, e)))?;

    //let mut size = 0_i32;
    //if let Err(e) = unsafe { hidraw_ioc_grdescsize(fd.as_raw_fd(), &mut size) } {
    //    return Err(HidError::HidApiError {
    //        message: format!("ioctl(GRDESCSIZE) error for {path}, not a HIDRAW device?: {e}"),
    //    });
    //}

    Ok(BackendDevice {
        fd,
    })
}

pub type BackendDeviceId = PathBuf;
pub type BackendError = std::io::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}


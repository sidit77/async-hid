mod descriptor;
mod ioctl;
mod utils;

use std::fs::{OpenOptions, read_dir, read_to_string};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use futures_core::Stream;

use nix::fcntl::OFlag;
use nix::unistd::{read, write};
use tokio::io::unix::AsyncFd;
use tokio::io::Interest;

use crate::backend::hidraw::descriptor::HidrawReportDescriptor;
use crate::backend::hidraw::ioctl::hidraw_ioc_grdescsize;
use crate::{ensure, DeviceInfo, ErrorSource, HidError, HidResult, AccessMode};
use crate::backend::hidraw::utils::{iter, TryIterExt};

pub async fn enumerate() -> HidResult<impl Stream<Item = DeviceInfo>> {
    let devices = read_dir("/sys/class/hidraw/")?
        .map(|r| r.map(|e| e.path()))
        .try_collect_vec()?;
    let devices = devices
        .into_iter()
        .map(get_device_info_raw)
        .filter_map(|r| {
            r.map_err(|e| log::trace!("Failed to query device information\n\tbecause {e:?}"))
                .ok()
        })
        .flatten();
    Ok(iter(devices))
}

fn get_device_info_raw(path: PathBuf) -> HidResult<Vec<DeviceInfo>> {
    let properties = read_to_string(path.join("uevent"))?;
    let id = read_property(&properties, "DEVNAME")
        .ok_or(HidError::custom("Can't find dev name"))
        .and_then(mange_dev_name)?;

    let properties = read_to_string(path.join("device/uevent"))?;

    let (_bus, vendor_id, product_id) = read_property(&properties, "HID_ID")
        .and_then(parse_hid_vid_pid)
        .ok_or(HidError::custom("Can't find hid ids"))?;

    let name = read_property(&properties, "HID_NAME")
        .ok_or(HidError::custom("Can't find hid name"))?
        .to_string();

    let info = DeviceInfo {
        id: id.into(),
        name,
        product_id,
        vendor_id,
        usage_id: 0,
        usage_page: 0,
    };

    let results = HidrawReportDescriptor::from_syspath(&path)
        .map(|descriptor| {
            descriptor
                .usages()
                .map(|(usage_page, usage_id)| DeviceInfo {
                    usage_page,
                    usage_id,
                    ..info.clone()
                })
                .collect()
        })
        .unwrap_or_else(|_| vec![info]);
    Ok(results)
}

fn read_property<'a>(properties: &'a str, key: &str) -> Option<&'a str> {
    properties
        .lines()
        .filter_map(|l| l.split_once('='))
        .find_map(|(k, v)| (k == key).then_some(v))
}

fn mange_dev_name(dev_name: &str) -> HidResult<PathBuf> {
    let path = Path::new(dev_name);
    if path.is_absolute() {
        ensure!(
            dev_name.strip_prefix("/dev/").is_some_and(|z| !z.is_empty()),
            HidError::custom("Absolute device paths must start with /dev/"));
        Ok(path.to_path_buf())
    } else {
        Ok(Path::new("/dev/").join(path))
    }
}


fn parse_hid_vid_pid(s: &str) -> Option<(u16, u16, u16)> {
    let mut elems = s.split(':').filter_map(|s| u16::from_str_radix(s, 16).ok());
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
        self.fd
            .async_io(Interest::READABLE, |fd| read(fd.as_raw_fd(), buf).map_err(BackendError::from))
            .await
            .map_err(HidError::from)
    }

    pub async fn write_output_report(&self, data: &[u8]) -> HidResult<()> {
        ensure!(!data.is_empty(), HidError::zero_sized_data());
        self.fd
            .async_io(Interest::WRITABLE, |fd| write(fd.as_raw_fd(), data).map_err(BackendError::from))
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

    Ok(BackendDevice { fd: AsyncFd::new(fd)? })
}

pub type BackendDeviceId = PathBuf;
pub type BackendError = std::io::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}

/*
udev device searching

pub async fn enumerate() -> HidResult<impl Stream<Item = DeviceInfo>> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_subsystem("hidraw")?;
    let devices: Vec<Device> = enumerator
        .scan_devices()?
        .collect();
    let devices = devices
        .into_iter()
        .map(get_device_info)
        .filter_map(|r| {
            r.map_err(|e| log::trace!("Failed to query device information\n\tbecause {e:?}"))
                .ok()
        })
        .flatten();
    Ok(iter(devices))
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
      .map(|descriptor| {
          descriptor
              .usages()
              .map(|(usage_page, usage_id)| DeviceInfo {
                  usage_page,
                  usage_id,
                  ..info.clone()
              })
              .collect()
      })
      .unwrap_or_else(|_| vec![info]);
  Ok(results)
}
*/
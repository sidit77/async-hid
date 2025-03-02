mod descriptor;
mod ioctl;
mod utils;

use std::fs::{OpenOptions, read_dir, read_to_string};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use futures_core::Stream;
use nix::fcntl::OFlag;
use nix::unistd::{read, write};

use crate::backend::hidraw::descriptor::HidrawReportDescriptor;
use crate::backend::hidraw::utils::{iter, TryIterExt};
use crate::{ensure, DeviceInfo, HidError, HidResult, Backend, AsyncHidRead, AsyncHidWrite};

use crate::backend::hidraw::async_api::{AsyncFd, read_with, write_with};
use crate::backend::hidraw::ioctl::hidraw_ioc_grdescsize;

#[derive(Clone)]
pub struct HidRawBackend;

impl Backend for HidRawBackend {
    type DeviceId = PathBuf;
    type Reader = HidDevice;
    type Writer = HidDevice;

    async fn enumerate() -> HidResult<impl Stream<Item=DeviceInfo<Self>> + Unpin + Send> {
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

    async fn open(id: &Self::DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        let fd: OwnedFd = OpenOptions::new()
            .read(read)
            .write(write)
            .custom_flags((OFlag::O_CLOEXEC | OFlag::O_NONBLOCK).bits())
            .open(id)?
            .into();

        let mut size = 0i32;
        unsafe { hidraw_ioc_grdescsize(fd.as_raw_fd(), &mut size) }
            .map_err(|e| HidError::message(format!("ioctl(GRDESCSIZE) error for {:?}, not a HIDRAW device?: {}", id, e)))?;
        
        let device = HidDevice(Arc::new(AsyncFd::new(fd)?));
        
        Ok((read.then(|| device.clone()), write.then(|| device.clone())))
    }
}

fn get_device_info_raw(path: PathBuf) -> HidResult<Vec<DeviceInfo>> {
    let properties = read_to_string(path.join("uevent"))?;
    let id = read_property(&properties, "DEVNAME")
        .ok_or(HidError::message("Can't find dev name"))
        .and_then(mange_dev_name)?;

    let properties = read_to_string(path.join("device/uevent"))?;

    let (_bus, vendor_id, product_id) = read_property(&properties, "HID_ID")
        .and_then(parse_hid_vid_pid)
        .ok_or(HidError::message("Can't find hid ids"))?;

    let name = read_property(&properties, "HID_NAME")
        .ok_or(HidError::message("Can't find hid name"))?
        .to_string();

    let serial_number = read_property(&properties, "HID_UNIQ")
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let info = DeviceInfo {
        id: id.into(),
        name,
        product_id,
        vendor_id,
        usage_id: 0,
        usage_page: 0,
        serial_number,
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
            dev_name
                .strip_prefix("/dev/")
                .is_some_and(|z| !z.is_empty()),
            HidError::message("Absolute device paths must start with /dev/")
        );
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

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct HidDevice(Arc<AsyncFd>);

impl AsyncHidRead for HidDevice {
    async fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
        read_with(&self.0, |fd| read(fd.as_raw_fd(), buf).map_err(std::io::Error::from))
            .await
            .map_err(HidError::from)
    }
}

impl AsyncHidWrite for HidDevice {
    async fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        write_with(&self.0, |fd| write(fd.as_raw_fd(), buf).map_err(std::io::Error::from))
            .await
            .map_err(HidError::from)
            .map(|i| debug_assert_eq!(i, buf.len()))
    }
}

#[cfg(all(feature = "async-io", feature = "tokio"))]
compile_error!("Only tokio or async-io can be active at the same time");

#[cfg(feature = "async-io")]
mod async_api {
    use std::os::fd::OwnedFd;
    use async_io::Async;

    pub type AsyncFd = Async<OwnedFd>;

    pub async fn read_with<R>(inner: &AsyncFd, op: impl FnMut(&OwnedFd) -> std::io::Result<R>) -> std::io::Result<R> {
        inner.read_with(op).await
    }

    pub async fn write_with<R>(inner: &AsyncFd, op: impl FnMut(&OwnedFd) -> std::io::Result<R>) -> std::io::Result<R> {
        inner.write_with(op).await
    }
}

#[cfg(feature = "tokio")]
mod async_api {
    use std::os::fd::OwnedFd;
    use tokio::io::Interest;

    pub type AsyncFd = tokio::io::unix::AsyncFd<OwnedFd>;

    pub async fn read_with<R>(inner: &AsyncFd, op: impl FnMut(&OwnedFd) -> std::io::Result<R>) -> std::io::Result<R> {
        inner.async_io(Interest::READABLE, op).await
    }

    pub async fn write_with<R>(inner: &AsyncFd, op: impl FnMut(&OwnedFd) -> std::io::Result<R>) -> std::io::Result<R> {
        inner.async_io(Interest::WRITABLE, op).await
    }
}


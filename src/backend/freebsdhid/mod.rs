// FreeBSD hidraw(4) backend.
//
// Talks to /dev/hidraw* via the FreeBSD-native ioctls in <dev/hid/hidraw.h>.
// Requires: FreeBSD 13+, hw.usb.usbhid.enable=1, hidraw kernel module loaded
// (hidraw_load="YES" in loader.conf).
//
// Nodes are 0600 root:operator by default; add a devfs.rules(5) entry to
// grant access to your user.
//
// Both the `async-io` and `tokio` features work. hidraw(4) supports only
// the EVFILT_READ kqueue filter, which constrains the tokio path; see the
// `async_api` module at the bottom of this file.

mod descriptor;
mod devd;
mod ioctl;

use std::ffi::OsStr;
use std::fs::{read_dir, OpenOptions};
use std::io::ErrorKind;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_lite::stream::{iter, unfold, Boxed};
use futures_lite::StreamExt;
use nix::libc::EIO;
use nix::unistd::{read, write};

use crate::backend::freebsdhid::devd::{parse_line, ParsedEvent};

use crate::backend::freebsdhid::async_api::{read_with, write_with, AsyncFd};
use crate::backend::freebsdhid::descriptor::HidrawReportDescriptor;
use crate::backend::freebsdhid::ioctl::{
    cstr_from_bytes, hidiocg_rdesc, hidiocg_rdescsize, hidraw_get_deviceinfo, hidraw_get_feature,
    hidraw_set_feature, HidrawDeviceInfo, HidrawReportDescriptor as HidrawReportDescriptorRaw,
    HID_MAX_DESCRIPTOR_SIZE,
};
use crate::backend::{Backend, DeviceInfoStream};
use crate::traits::{AsyncHidFeatureHandle, AsyncHidRead, AsyncHidWrite};
use crate::{ensure, DeviceEvent, DeviceId, DeviceInfo, HidError, HidResult};

#[derive(Default)]
pub struct FreeBsdHidBackend;

impl Backend for FreeBsdHidBackend {
    type Reader = HidDevice;
    type Writer = HidDevice;
    type FeatureHandle = HidDevice;

    async fn enumerate(&self) -> HidResult<DeviceInfoStream> {
        let mut paths: Vec<PathBuf> = read_dir("/dev")?
            .filter_map(|entry| entry.ok())
            .map(|e| e.path())
            .filter(|p| is_hidraw_node(p))
            .collect();
        paths.sort();

        let infos = paths.into_iter().flat_map(|path| match get_device_info(&path) {
            Ok(infos) => infos.into_iter().map(Ok).collect::<Vec<_>>(),
            Err(err) => {
                log::debug!("Failed to enumerate {}: {}", path.display(), err);
                Vec::new()
            }
        });

        Ok(iter(infos).boxed())
    }

    fn watch(&self) -> HidResult<Boxed<DeviceEvent>> {
        const DEVD_SOCKET: &str = "/var/run/devd.pipe";

        let stream = UnixStream::connect(DEVD_SOCKET)
            .map_err(|e| HidError::message(format!("connect {}: {}", DEVD_SOCKET, e)))?;
        stream.set_nonblocking(true)?;
        // Go through the runtime-agnostic `async_api` wrapper so both the
        // `async-io` and `tokio` features build.
        let socket: OwnedFd = stream.into();

        // State: (async socket, tail bytes not yet forming a full line, read buffer).
        let state = (AsyncFd::new(socket)?, String::new(), vec![0u8; 4096]);
        Ok(unfold(state, |(sock, mut tail, mut buf)| async move {
            loop {
                // Drain any lines already buffered in `tail` before touching the socket.
                if let Some((event, remainder)) = extract_next(&tail) {
                    tail = remainder;
                    if let Some(ev) = event {
                        return Some((ev, (sock, tail, buf)));
                    }
                    continue;
                }

                let n = match read_with(&sock, |fd| read(fd.as_raw_fd(), &mut buf).map_err(std::io::Error::from)).await {
                    Ok(0) => {
                        log::debug!("devd closed the socket");
                        return None;
                    }
                    Ok(n) => n,
                    Err(err) => {
                        log::warn!("devd read failed: {}", err);
                        return None;
                    }
                };
                tail.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        })
        .boxed())
    }

    async fn query_info(&self, id: &DeviceId) -> HidResult<Vec<DeviceInfo>> {
        let DeviceId::DevPath(path) = id;
        get_device_info(path)
    }

    async fn open(&self, id: &DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        let DeviceId::DevPath(path) = id;

        let fd: OwnedFd = OpenOptions::new()
            .read(read)
            .write(write)
            .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
            .open(path)
            .map_err(|err| match err {
                err if err.kind() == ErrorKind::NotFound => HidError::NotConnected,
                err => err.into(),
            })?
            .into();

        // Sanity-check we opened an actual hidraw node.
        let mut desc_size = 0i32;
        unsafe { hidiocg_rdescsize(fd.as_raw_fd(), &mut desc_size) }
            .map_err(|e| HidError::message(format!("ioctl(GRDESCSIZE) on {}: {}", path.display(), e)))?;

        let device = HidDevice(Arc::new(AsyncFd::new(fd)?));
        Ok((read.then(|| device.clone()), write.then(|| device.clone())))
    }

    async fn open_feature_handle(&self, id: &DeviceId) -> HidResult<Self::FeatureHandle> {
        let (_, writer) = self.open(id, true, true).await?;
        writer.ok_or(HidError::message("Failed to open device for feature report"))
    }
}

/// Pop one line off `tail`. Returns Some((event_opt, remainder)) if a full
/// line was present, None if we need more bytes from the socket. event_opt
/// is None for lines that don't match a hidraw attach/detach.
fn extract_next(tail: &str) -> Option<(Option<DeviceEvent>, String)> {
    let nl = tail.find('\n')?;
    let (line, rest) = tail.split_at(nl);
    let remainder = rest[1..].to_string(); // skip the '\n' itself
    let event = parse_line(line.trim_end_matches('\r')).map(|ev| match ev {
        ParsedEvent::Attach(p) => DeviceEvent::Connected(DeviceId::DevPath(p)),
        ParsedEvent::Detach(p) => DeviceEvent::Disconnected(DeviceId::DevPath(p)),
    });
    Some((event, remainder))
}

fn is_hidraw_node(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .map(|name| {
            name.strip_prefix("hidraw")
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        })
        .unwrap_or(false)
}

fn get_device_info(path: &Path) -> HidResult<Vec<DeviceInfo>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_CLOEXEC)
        .open(path)?;
    let fd = file.as_raw_fd();

    let mut info: MaybeUninit<HidrawDeviceInfo> = MaybeUninit::zeroed();
    let info = unsafe {
        hidraw_get_deviceinfo(fd, info.as_mut_ptr())?;
        info.assume_init()
    };

    let name = cstr_from_bytes(&info.hdi_name).unwrap_or_default();
    let serial_number = cstr_from_bytes(&info.hdi_uniq).filter(|s| !s.is_empty());

    let base = DeviceInfo {
        id: DeviceId::DevPath(path.to_path_buf()),
        name,
        manufacturer: None,
        product_id: info.hdi_product,
        vendor_id: info.hdi_vendor,
        usage_id: 0,
        usage_page: 0,
        serial_number,
    };

    let mut desc_size = 0i32;
    let desc_size = match unsafe { hidiocg_rdescsize(fd, &mut desc_size) } {
        Ok(_) => desc_size.clamp(0, (HID_MAX_DESCRIPTOR_SIZE - 1) as i32) as u32,
        Err(err) => {
            log::debug!("HIDIOCGRDESCSIZE on {} failed: {}", path.display(), err);
            return Ok(vec![base]);
        }
    };

    let mut desc = Box::new(HidrawReportDescriptorRaw::default());
    desc.size = desc_size;
    let results = match unsafe { hidiocg_rdesc(fd, &mut desc) } {
        Ok(()) => {
            let bytes = &desc.value[..desc.size as usize];
            HidrawReportDescriptor::from_slice(bytes)
                .map(|d| {
                    d.usages()
                        .map(|(usage_page, usage_id)| DeviceInfo {
                            usage_page,
                            usage_id,
                            ..base.clone()
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|_| vec![base.clone()])
        }
        Err(err) => {
            log::debug!("HIDIOCGRDESC on {} failed: {}", path.display(), err);
            vec![base.clone()]
        }
    };

    Ok(if results.is_empty() { vec![base] } else { results })
}

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct HidDevice(Arc<AsyncFd>);

impl AsyncHidRead for HidDevice {
    async fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
        read_with(&self.0, |fd| read(fd.as_raw_fd(), buf).map_err(std::io::Error::from))
            .await
            .map_err(|err| match err {
                err if err.raw_os_error() == Some(EIO) => HidError::Disconnected,
                err => err.into(),
            })
    }
}

impl AsyncHidWrite for HidDevice {
    async fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        write_with(&self.0, |fd| write(fd, buf).map_err(std::io::Error::from))
            .await
            .map_err(|err| match err {
                err if err.raw_os_error() == Some(EIO) => HidError::Disconnected,
                err => err.into(),
            })
            .map(|i| debug_assert_eq!(i, buf.len()))
    }
}

impl AsyncHidFeatureHandle for HidDevice {
    async fn read_feature_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
        ensure!(!buf.is_empty(), HidError::message("Buffer cannot be empty"));

        let result = write_with(&self.0, |fd| {
            unsafe { hidraw_get_feature(fd.as_raw_fd(), buf) }.map_err(std::io::Error::from)
        })
        .await
        .map_err(|e| HidError::message(format!("ioctl(GFEATURE) error: {}", e)))?;

        Ok(result as usize)
    }

    async fn write_feature_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        ensure!(!buf.is_empty(), HidError::message("Buffer cannot be empty"));

        let data = buf.to_vec();
        write_with(&self.0, |fd| {
            unsafe { hidraw_set_feature(fd.as_raw_fd(), &data) }.map_err(std::io::Error::from)
        })
        .await
        .map_err(|e| HidError::message(format!("ioctl(SFEATURE) error: {}", e)))?;

        Ok(())
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

// FreeBSD's hidraw(4) implements only the EVFILT_READ kqueue filter and
// rejects EVFILT_WRITE with EINVAL (sys/dev/hid/hidraw.c, hidraw_kqfilter).
// That shapes this whole module:
//
//   * `AsyncFd::new` requests read+write interest, and mio submits both
//     filters in one kevent() call, so it always fails. Register with
//     `with_interest(READABLE)` instead.
//   * A WRITABLE readiness wait never completes, so `write_with` must not
//     await one. Writes to a hidraw node never block (hidraw_write calls
//     hid_write synchronously, with no output queue), and hidraw_poll
//     already reports POLLOUT unconditionally, so running the op directly
//     is correct rather than merely expedient.
#[cfg(feature = "tokio")]
mod async_api {
    use std::os::fd::OwnedFd;

    use tokio::io::Interest;

    pub struct AsyncFd(tokio::io::unix::AsyncFd<OwnedFd>);

    impl AsyncFd {
        pub fn new(fd: OwnedFd) -> std::io::Result<Self> {
            tokio::io::unix::AsyncFd::with_interest(fd, Interest::READABLE).map(Self)
        }
    }

    impl std::fmt::Debug for AsyncFd {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AsyncFd").finish_non_exhaustive()
        }
    }

    pub async fn read_with<R>(inner: &AsyncFd, op: impl FnMut(&OwnedFd) -> std::io::Result<R>) -> std::io::Result<R> {
        inner.0.async_io(Interest::READABLE, op).await
    }

    pub async fn write_with<R>(inner: &AsyncFd, mut op: impl FnMut(&OwnedFd) -> std::io::Result<R>) -> std::io::Result<R> {
        op(inner.0.get_ref())
    }
}

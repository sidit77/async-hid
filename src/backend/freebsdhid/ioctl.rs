// FreeBSD hidraw(4) ioctls. See /usr/include/dev/hid/hidraw.h.
//
// Group is 'U' (historical uhid), not Linux's 'H' — ioctl numbers do not
// match Linux's <linux/hidraw.h>.

use std::os::fd::RawFd;

use nix::errno::Errno;
use nix::libc;
use nix::{ioctl_read, ioctl_readwrite_buf, ioctl_write_buf, request_code_none};

pub const HID_MAX_DESCRIPTOR_SIZE: usize = 4096;

#[repr(C)]
pub struct HidrawReportDescriptor {
    pub size: u32,
    pub value: [u8; HID_MAX_DESCRIPTOR_SIZE],
}

impl Default for HidrawReportDescriptor {
    fn default() -> Self {
        Self { size: 0, value: [0; HID_MAX_DESCRIPTOR_SIZE] }
    }
}

/// _IO('U', 31): pointer to a HidrawReportDescriptor. Kernel reads `size`,
/// writes back that many bytes into `value`. Not `_IOWR` in the header, so
/// we compose the request number by hand and call libc::ioctl.
pub unsafe fn hidiocg_rdesc(fd: RawFd, desc: &mut HidrawReportDescriptor) -> Result<(), Errno> {
    let req = request_code_none!(b'U', 31) as libc::c_ulong;
    let rc = libc::ioctl(fd, req, desc as *mut HidrawReportDescriptor);
    if rc < 0 {
        Err(Errno::last())
    } else {
        Ok(())
    }
}


/// One-shot device info: VID/PID/version/bustype + name/phys/uniq strings.
/// Preferred over the Linux-compat trio (HIDIOCGRAWINFO + GRAWNAME + GRAWUNIQ).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct HidrawDeviceInfo {
    pub hdi_product: u16,
    pub hdi_vendor: u16,
    pub hdi_version: u16,
    pub occupied: [u8; 18],
    pub hdi_bustype: u16,
    pub reserved: [u8; 14],
    pub hdi_name: [u8; 128],
    pub hdi_phys: [u8; 128],
    pub hdi_uniq: [u8; 64],
    pub hdi_release: [u8; 8],
}

ioctl_read!(hidraw_get_deviceinfo, b'U', 112, HidrawDeviceInfo);

// Linux-compat: descriptor size in bytes.
ioctl_read!(hidiocg_rdescsize, b'U', 30, i32);

// HIDIOCSFEATURE(len) and HIDIOCGFEATURE(len) from <dev/hid/hidraw.h>.
// Buffer discipline matches Linux hidraw: first byte is report id (0 for
// unnumbered devices), payload starts at byte 1, minimum len = 2.
ioctl_write_buf!(hidraw_set_feature, b'U', 35, u8);
ioctl_readwrite_buf!(hidraw_get_feature, b'U', 36, u8);

/// Read a NUL-terminated C string field, up to its length, decoded as UTF-8.
pub fn cstr_from_bytes(buf: &[u8]) -> Option<String> {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    if end == 0 {
        return None;
    }
    std::str::from_utf8(&buf[..end]).ok().map(str::to_owned)
}

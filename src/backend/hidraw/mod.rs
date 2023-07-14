use std::path::PathBuf;

use udev::{Device, Enumerator};

use crate::{DeviceInfo, ErrorSource, HidResult};

pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_subsystem("hidraw")?;
    let devices = enumerator
        .scan_devices()?
        .filter_map(|dev| get_device_info(&dev))
        .flatten()
        .collect();
    Ok(devices)
}

fn get_device_info(raw_device: &Device) -> Option<Vec<DeviceInfo>> {
    let device = raw_device.parent_with_subsystem("hid").unwrap().unwrap();

    let (_bus, vendor_id, product_id) = device
        .property_value("HID_ID")
        .and_then(|s| s.to_str())
        .and_then(parse_hid_vid_pid)
        .unwrap();

    let id = raw_device.devnode().unwrap().to_path_buf();

    let name = device
        .property_value("HID_NAME")
        .unwrap()
        .to_string_lossy()
        .to_string();

    Some(vec![DeviceInfo {
        id: id.into(),
        name,
        product_id,
        vendor_id,
        usage_id: 0,
        usage_page: 0
    }])
}

fn parse_hid_vid_pid(s: &str) -> Option<(u16, u16, u16)> {
    let mut elems = s.split(':').map(|s| u16::from_str_radix(s, 16));
    let devtype = elems.next()?.ok()?;
    let vendor = elems.next()?.ok()?;
    let product = elems.next()?.ok()?;

    Some((devtype, vendor, product))
}

#[derive(Debug, Clone)]
pub struct BackendDevice;

impl BackendDevice {
    pub async fn read_input_report(&self, _buf: &mut [u8]) -> HidResult<usize> {
        Ok(0)
    }
}

pub async fn open(_id: &BackendDeviceId) -> HidResult<BackendDevice> {
    Ok(BackendDevice)
}

pub type BackendDeviceId = PathBuf;
pub type BackendError = std::io::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}

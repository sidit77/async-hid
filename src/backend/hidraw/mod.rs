mod descriptor;

use std::path::PathBuf;

use udev::{Device, Enumerator};

use crate::{DeviceInfo, ErrorSource, HidError, HidResult};
use crate::backend::hidraw::descriptor::HidrawReportDescriptor;

pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_subsystem("hidraw")?;
    let devices = enumerator
        .scan_devices()?
        .filter_map(|dev| get_device_info(&dev).ok())
        .flatten()
        .collect();
    Ok(devices)
}

fn get_device_info(raw_device: &Device) -> HidResult<Vec<DeviceInfo>> {
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

#[derive(Debug, Clone)]
pub struct BackendDevice;

impl BackendDevice {
    pub async fn read_input_report(&self, _buf: &mut [u8]) -> HidResult<usize> {
        Ok(0)
    }
    pub async fn write_output_report(&self, _buf: &[u8]) -> HidResult<()> { Ok(()) }
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


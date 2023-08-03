mod manager;
mod device;
mod service;

use io_kit_sys::hid::keys::{kIOHIDProductIDKey, kIOHIDProductKey, kIOHIDVendorIDKey};
use crate::{AccessMode, DeviceInfo, ErrorSource, HidResult};
use crate::backend::iohidmanager::device::IOHIDDevice;
use crate::backend::iohidmanager::manager::IOHIDManager;
use crate::backend::iohidmanager::service::IOService;

pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
    let mut manager = IOHIDManager::new()?;
    let devices = manager
        .get_devices()?
        .iter()
        .map(get_device_infos)
        .filter_map(Result::ok)
        .flatten()
        .collect();

    Ok(devices)
}

fn get_device_infos(device: &IOHIDDevice) -> HidResult<Vec<DeviceInfo>> {
    let vendor_id = device.get_i32_property(kIOHIDVendorIDKey)? as u16;
    let product_id = device.get_i32_property(kIOHIDProductIDKey)? as u16;
    let name = device.get_string_property(kIOHIDProductKey)?;
    let id = IOService::try_from(device)
        .and_then(|i| i.get_registry_entry_id())
        .map(|id| format!("DevSrvsID:{id}"))?;

    println!("device: {} {} {:x} {:x}", id, name, vendor_id, product_id);

    let info = DeviceInfo {
        id: id.into(),
        name,
        product_id,
        vendor_id,
        usage_id: 0,
        usage_page: 0,
    };
    
    Ok(Vec::new())
}

#[derive(Debug, Clone)]
pub struct BackendDevice;


pub async fn open(_id: &BackendDeviceId, _mode: AccessMode) -> HidResult<BackendDevice> {
    unimplemented!()
}

impl BackendDevice {
    pub async fn read_input_report(&self, _buf: &mut [u8]) -> HidResult<usize> {
        unimplemented!()
    }

    pub async fn write_output_report(&self, _buf: &[u8]) -> HidResult<()> {
        unimplemented!()
    }
}

pub type BackendDeviceId = String;
pub type BackendError = ();

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}
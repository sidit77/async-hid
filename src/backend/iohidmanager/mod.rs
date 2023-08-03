mod manager;
mod device;
mod service;
mod utils;

use core_foundation::array::CFArray;
use core_foundation::base::{TCFType};
use core_foundation::dictionary::{CFDictionary};
use io_kit_sys::hid::keys::*;
use crate::{AccessMode, DeviceInfo, ErrorSource, HidResult};
use crate::backend::iohidmanager::device::IOHIDDevice;
use crate::backend::iohidmanager::manager::IOHIDManager;
use crate::backend::iohidmanager::service::IOService;
use crate::backend::iohidmanager::utils::CFDictionaryExt;

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
    let primary_usage_page = device .get_i32_property(kIOHIDPrimaryUsagePageKey)? as u16;
    let primary_usage = device.get_i32_property(kIOHIDPrimaryUsageKey)? as u16;
    let vendor_id = device.get_i32_property(kIOHIDVendorIDKey)? as u16;
    let product_id = device.get_i32_property(kIOHIDProductIDKey)? as u16;
    let name = device.get_string_property(kIOHIDProductKey)?;
    let id = IOService::try_from(device)
        .and_then(|i| i.get_registry_entry_id())
        .map(|id| format!("DevSrvsID:{id}"))?;

    let info = DeviceInfo {
        id: id.into(),
        name,
        product_id,
        vendor_id,
        usage_id: primary_usage,
        usage_page: primary_usage_page,
    };

    let mut results = Vec::new();
    results.extend(device
        .property::<CFArray>(kIOHIDDeviceUsagePairsKey)?
        .iter()
        .map(|i| unsafe { CFDictionary::wrap_under_get_rule(*i as _) })
        .filter_map(|dict| {
            let usage = dict.lookup_i32(kIOHIDDeviceUsageKey).ok()? as u16;
            let usage_page = dict.lookup_i32(kIOHIDDeviceUsagePageKey).ok()? as u16;
            Some((usage, usage_page))
        })
        .filter(|(usage, usage_page)| (*usage_page != primary_usage_page) || (*usage != primary_usage))
        .map(|(usage_id, usage_page)| DeviceInfo {
            usage_id,
            usage_page,
            ..info.clone()
        }));
    results.push(info.clone());

    Ok(results)
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
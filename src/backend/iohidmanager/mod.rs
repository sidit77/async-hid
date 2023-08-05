mod manager;
mod device;
mod service;
mod utils;
mod runloop;

use std::cell::RefCell;
use std::sync::Arc;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use core_foundation::array::CFArray;
use core_foundation::base::{TCFType};
use core_foundation::dictionary::{CFDictionary};
use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
use core_foundation::string::CFString;
use io_kit_sys::hid::keys::*;
use io_kit_sys::hid::usage_tables::kHIDUsage_Csmr_ACJustifyBlockV;
use io_kit_sys::types::IOOptionBits;
use tokio::sync::mpsc::{channel, Receiver};
use crate::{AccessMode, DeviceInfo, ErrorSource, HidResult};
use crate::backend::iohidmanager::device::{CallbackGuard, IOHIDDevice};
use crate::backend::iohidmanager::manager::IOHIDManager;
use crate::backend::iohidmanager::runloop::RunLoop;
use crate::backend::iohidmanager::service::{IOService, RegistryEntryId};
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
        .and_then(|i| i.get_registry_entry_id())?;

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


pub struct BackendDevice {
    device: IOHIDDevice,
    open_options: IOOptionBits,
    run_loop: Arc<RunLoop>,
    _callback: CallbackGuard,
    read_channel: RefCell<Receiver<Bytes>>
}

impl Drop for BackendDevice {
    fn drop(&mut self) {
        self.run_loop.unschedule_device(&self.device).unwrap();
        let default_mode = unsafe { CFString::wrap_under_create_rule(kCFRunLoopDefaultMode) };
        self.device.schedule_with_runloop(&CFRunLoop::get_main(), &default_mode);
        self.device.close(self.open_options).unwrap();
    }
}

pub async fn open(id: &BackendDeviceId, _mode: AccessMode) -> HidResult<BackendDevice> {
    let open_options = 0;
    let device = IOHIDDevice::try_from(*id)?;
    device.open(open_options)?;

    let mut byte_buffer = BytesMut::with_capacity(1024);
    let (sender, receiver) = channel(64);

    let callback = device.register_input_report_callback(move |report| {
        byte_buffer.put(report);
        let bytes = byte_buffer.split().freeze();
        sender.try_send(bytes).unwrap();
        //println!("{:?}", report)
    })?;
    let run_loop = RunLoop::new().await?;
    run_loop.schedule_device(&device)?;

    Ok(BackendDevice {
        device,
        open_options,
        run_loop: Arc::new(run_loop),
        _callback: callback,
        read_channel: RefCell::new(receiver),
    })
}

impl BackendDevice {
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        let bytes = self.read_channel.borrow_mut().recv().await.unwrap();
        let length = bytes.len().min(buf.len());
        buf[..length].copy_from_slice(&bytes[..length]);
        Ok(length)
    }

    pub async fn write_output_report(&self, _buf: &[u8]) -> HidResult<()> {
        unimplemented!()
    }
}

pub type BackendDeviceId = RegistryEntryId;
pub type BackendError = ();

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}
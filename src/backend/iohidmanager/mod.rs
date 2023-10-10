mod device;
mod manager;
mod runloop;
mod service;
mod utils;

use std::sync::Arc;

use async_channel::{bounded, Receiver, TrySendError};
use bytes::{BufMut, Bytes, BytesMut};
use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
use core_foundation::string::CFString;
use futures_core::Stream;
use io_kit_sys::hid::keys::*;
use io_kit_sys::types::IOOptionBits;

use crate::backend::iohidmanager::device::{CallbackGuard, IOHIDDevice};
use crate::backend::iohidmanager::manager::IOHIDManager;
use crate::backend::iohidmanager::runloop::RunLoop;
use crate::backend::iohidmanager::service::{IOService, RegistryEntryId};
use crate::backend::iohidmanager::utils::{iter, CFDictionaryExt};
use crate::{ensure, AccessMode, DeviceInfo, ErrorSource, HidError, HidResult};

pub async fn enumerate() -> HidResult<impl Stream<Item = DeviceInfo> + Send + Unpin> {
    let mut manager = IOHIDManager::new()?;
    let devices = manager
        .get_devices()?
        .into_iter()
        .map(get_device_infos)
        .filter_map(|r| {
            r.map_err(|e| log::trace!("Failed to query device information\n\tbecause {e:?}"))
                .ok()
        })
        .flatten();

    Ok(iter(devices))
}

fn get_device_infos(device: IOHIDDevice) -> HidResult<Vec<DeviceInfo>> {
    let primary_usage_page = device.get_i32_property(kIOHIDPrimaryUsagePageKey)? as u16;
    let primary_usage = device.get_i32_property(kIOHIDPrimaryUsageKey)? as u16;
    let vendor_id = device.get_i32_property(kIOHIDVendorIDKey)? as u16;
    let product_id = device.get_i32_property(kIOHIDProductIDKey)? as u16;
    let name = device.get_string_property(kIOHIDProductKey)?;
    let id = IOService::try_from(&device).and_then(|i| i.get_registry_entry_id())?;

    let info = DeviceInfo {
        id: id.into(),
        name,
        product_id,
        vendor_id,
        usage_id: primary_usage,
        usage_page: primary_usage_page,
        private_data: BackendPrivateData {}
    };

    let mut results = Vec::new();
    results.extend(
        device
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
            })
    );
    results.push(info.clone());

    Ok(results)
}

struct InputReceiver {
    run_loop: Arc<RunLoop>,
    _callback: CallbackGuard,
    read_channel: Receiver<Bytes>
}

impl InputReceiver {
    async fn new(device: &IOHIDDevice) -> HidResult<Self> {
        let mut byte_buffer = BytesMut::with_capacity(1024);
        let (sender, receiver) = bounded(64);

        let drain = receiver.clone();
        let callback = device.register_input_report_callback(move |report| {
            byte_buffer.put(report);
            let mut bytes = byte_buffer.split().freeze();
            while let Err(TrySendError::Full(ret)) = sender.try_send(bytes) {
                log::trace!("Dropping previous input report because the queue is full");
                let _ = drain.try_recv();
                bytes = ret;
            }
        })?;
        let run_loop = RunLoop::get_run_loop().await?;
        run_loop.schedule_device(&device)?;

        Ok(Self {
            run_loop,
            _callback: callback,
            read_channel: receiver
        })
    }

    fn stop(self, device: &IOHIDDevice) {
        self.run_loop
            .unschedule_device(device)
            .unwrap_or_else(|_| log::warn!("Failed to unschedule IOHIDDevice from run loop"));
        let default_mode = unsafe { CFString::wrap_under_create_rule(kCFRunLoopDefaultMode) };
        device.schedule_with_runloop(&CFRunLoop::get_main(), &default_mode);
    }

    async fn recv(&self) -> HidResult<Bytes> {
        self.read_channel
            .recv()
            .await
            .map_err(|_| HidError::custom("Input report callback got dropped unexpectedly"))
    }
}

pub struct BackendDevice {
    device: IOHIDDevice,
    open_options: IOOptionBits,
    input_receiver: Option<InputReceiver>
}

impl Drop for BackendDevice {
    fn drop(&mut self) {
        if let Some(input) = self.input_receiver.take() {
            input.stop(&self.device);
        }
        self.device
            .close(self.open_options)
            .unwrap_or_else(|err| log::warn!("Failed to close IOHIDDevice\n\t{err:?}"));
    }
}

pub async fn open(id: &BackendDeviceId, mode: AccessMode) -> HidResult<BackendDevice> {
    let open_options = 0;
    let device = IOHIDDevice::try_from(*id)?;
    device.open(open_options)?;

    let input_receiver = if mode.readable() {
        Some(InputReceiver::new(&device).await?)
    } else {
        None
    };

    Ok(BackendDevice {
        device,
        open_options,
        input_receiver
    })
}

impl BackendDevice {
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        ensure!(!buf.is_empty(), HidError::zero_sized_data());
        let bytes = self
            .input_receiver
            .as_ref()
            .expect("InputReceiver not active")
            .recv()
            .await?;
        let length = bytes.len().min(buf.len());
        buf[..length].copy_from_slice(&bytes[..length]);
        Ok(length)
    }

    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        ensure!(!buf.is_empty(), HidError::zero_sized_data());

        let report_id = buf[0];
        let data_to_send = if report_id == 0x0 { &buf[1..] } else { buf };

        self.device.set_report(1, report_id as _, data_to_send)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendPrivateData {}

pub type BackendDeviceId = RegistryEntryId;
pub type BackendError = ();

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}

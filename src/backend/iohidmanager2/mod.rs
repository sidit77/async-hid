mod device_info;

use std::ffi::c_void;
use crate::backend::iohidmanager2::device_info::get_device_info;
use crate::backend::{Backend, DeviceInfoStream};
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::utils::TryIterExt;
use crate::{DeviceEvent, DeviceId, HidResult};
use futures_lite::stream::{iter, pending, Boxed};
use futures_lite::{FutureExt, StreamExt};
use objc2_io_kit::{kIOReturnSuccess, IOHIDDevice, IOHIDManager, IOHIDManagerOptions, IOOptionBits, IOReturn};
use std::ptr::{null, null_mut, NonNull};
use dispatch2::{DispatchQueue, DispatchQueueAttr};

#[derive(Default)]
pub struct IoHidManagerBackend2;

impl Backend for IoHidManagerBackend2 {
    type Reader = DummyRW;
    type Writer = DummyRW;

    async fn enumerate(&self) -> HidResult<DeviceInfoStream> {
        let device_infos = unsafe {
            let manager = IOHIDManager::new(None, IOHIDManagerOptions::None.bits());
            manager.set_device_matching(None);


            let device_set = manager.devices().expect("Failed to get devices");
            let mut devices: Vec<NonNull<IOHIDDevice>> = vec![NonNull::dangling(); device_set.count() as usize];
            device_set.values(devices.as_mut_ptr().cast());
            devices
                .iter()
                .map(|d| get_device_info(d.as_ref()))
                .try_flatten()
                .collect::<Vec<_>>()
        };
        Ok(iter(device_infos).boxed())
    }

    fn watch(&self) -> HidResult<Boxed<DeviceEvent>> {
        unsafe {
            let queue = Box::leak(Box::new(DispatchQueue::new("async-hid", DispatchQueueAttr::SERIAL)));
            let manager = Box::leak(Box::new(IOHIDManager::new(None, IOHIDManagerOptions::None.bits())));
            manager.set_device_matching(None);

            manager.set_dispatch_queue(queue);
            manager.register_device_matching_callback(Some(added_callback), null_mut());
            manager.register_device_removal_callback(Some(removed_callback), null_mut());
            manager.activate();

            //assert_eq!(manager.open(IOHIDManagerOptions::None.bits()), kIOReturnSuccess);
        }
        
        println!("watching");
        Ok(pending().boxed())
    }

    async fn open(&self, _id: &DeviceId, _read: bool, _write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        todo!()
    }


}

unsafe extern "C-unwind" fn added_callback(_context: *mut c_void, _result: IOReturn, _sender: *mut c_void, _device: NonNull<IOHIDDevice>) {
    println!("DEVICE ADDED");
}

unsafe extern "C-unwind" fn removed_callback(_context: *mut c_void, _result: IOReturn, _sender: *mut c_void, _device: NonNull<IOHIDDevice>) {
    println!("DEVICE REMOVED");
}

#[derive(Debug)]
pub struct DummyRW;

impl AsyncHidRead for DummyRW {
    async fn read_input_report<'a>(&'a mut self, _buf: &'a mut [u8]) -> HidResult<usize> {
        todo!()
    }
}

impl AsyncHidWrite for DummyRW {
    async fn write_output_report<'a>(&'a mut self, _buf: &'a [u8]) -> HidResult<()> {
        todo!()
    }
}
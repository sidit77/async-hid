mod device_info;
mod read_writer;

use crate::backend::iohidmanager2::device_info::get_device_info;
use crate::backend::iohidmanager2::read_writer::DeviceReadWriter;
use crate::backend::{Backend, DeviceInfoStream};
use crate::utils::TryIterExt;
use crate::{ensure, DeviceEvent, DeviceId, HidError, HidResult};
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use futures_lite::stream::{iter, pending, Boxed};
use futures_lite::StreamExt;
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_io_kit::{kIOMasterPortDefault, kIOReturnSuccess, IOHIDDevice, IOHIDManager, IOHIDManagerOptions, IORegistryEntryIDMatching, IOReturn, IOServiceGetMatchingService};
use std::ffi::c_void;
use std::ptr::{null_mut, NonNull};
use std::sync::{Arc, LazyLock, Once};
use block2::RcBlock;
use log::trace;

static DISPATCH_QUEUE: LazyLock<DispatchRetained<DispatchQueue>> = LazyLock::new(|| DispatchQueue::new("async-hid", DispatchQueueAttr::SERIAL));

pub struct IoHidManagerBackend2{
    manager: CFRetained<IOHIDManager>
}

//SAFETY: IOHIDManager is immediately connected to a dispatch queue, and 
// all functions are called on that queue to the best of my knowledge
unsafe impl Send for IoHidManagerBackend2 {}
unsafe impl Sync for IoHidManagerBackend2 {}

impl Default for IoHidManagerBackend2 {
    fn default() -> Self {
        let manager = unsafe {
            let manager = IOHIDManager::new(None, IOHIDManagerOptions::None.bits());
            manager.set_dispatch_queue(&*DISPATCH_QUEUE);

            manager.set_device_matching(None);
            manager.register_device_matching_callback(Some(added_callback), null_mut());
            manager.register_device_removal_callback(Some(removed_callback), null_mut());
            manager.activate();
            manager
        };
        Self {
            manager
        }
    }
}

impl Drop for IoHidManagerBackend2 {
    fn drop(&mut self) {
        unsafe {
            let once = Arc::new(Once::new());
            let block = RcBlock::new({
                let once = once.clone();
                move || once.call_once(|| trace!("Finished canceling manager"))
            });

            self.manager.set_cancel_handler(RcBlock::as_ptr(&block));
            self.manager.cancel();
            trace!("Waiting for manager cancel to finish");
            once.wait();
            trace!("Resuming destructor of manager");
        }
    }
}

impl IoHidManagerBackend2 {
    
}

impl Backend for IoHidManagerBackend2 {
    type Reader = Arc<DeviceReadWriter>;
    type Writer = Arc<DeviceReadWriter>;

    async fn enumerate(&self) -> HidResult<DeviceInfoStream> {
        let device_infos = unsafe {
            let device_set = self.manager.devices().expect("Failed to get devices");
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
        /*
        unsafe {
            let queue = Box::leak(Box::new(DispatchQueue::new("async-hid", DispatchQueueAttr::SERIAL)));
            //let queue = Box::leak(Box::new(DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::Default))));
            let manager = Box::leak(Box::new(IOHIDManager::new(None, IOHIDManagerOptions::None.bits())));
            manager.set_dispatch_queue(queue);
            
            manager.set_device_matching(None);
            manager.register_device_matching_callback(Some(added_callback), null_mut());
            manager.register_device_removal_callback(Some(removed_callback), null_mut());
            manager.activate();

            //assert_eq!(manager.open(IOHIDManagerOptions::None.bits()), kIOReturnSuccess);
        }

        println!("watching");
        
         */
        Ok(pending().boxed())
    }

    async fn open(&self, id: &DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        let id = match id {
            DeviceId::RegistryEntryId(id) => *id
        };
        let device = unsafe {
            let service = IOServiceGetMatchingService(kIOMasterPortDefault, IORegistryEntryIDMatching(id).map(|d|d.downcast::<CFDictionary>().unwrap()));
            ensure!(service != 0, HidError::NotConnected);
            let device = IOHIDDevice::new(None, service).ok_or(HidError::message("Failed to create device"))?;
            device.set_dispatch_queue(&*DISPATCH_QUEUE);
            ensure!(device.open(DeviceReadWriter::DEVICE_OPTIONS) == kIOReturnSuccess, HidError::message("Failed to open device"));
            device
        };
        let rw = Arc::new(DeviceReadWriter::new(device, read, write)?);
        Ok((read.then_some(rw.clone()), write.then_some(rw)))
    }


}

unsafe extern "C-unwind" fn added_callback(_context: *mut c_void, _result: IOReturn, _sender: *mut c_void, device: NonNull<IOHIDDevice>) {
    println!("DEVICE ADDED: {:?}", get_device_info(device.as_ref()));
}

unsafe extern "C-unwind" fn removed_callback(_context: *mut c_void, _result: IOReturn, _sender: *mut c_void, device: NonNull<IOHIDDevice>) {
    println!("DEVICE REMOVED: {:?}", get_device_info(device.as_ref()));
}


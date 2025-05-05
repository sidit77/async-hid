mod device_info;

use crate::backend::iohidmanager2::device_info::{get_device_info, property_key};
use crate::backend::{Backend, DeviceInfoStream};
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::utils::TryIterExt;
use crate::{ensure, DeviceEvent, DeviceId, HidError, HidResult};
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use futures_lite::stream::{iter, pending, Boxed};
use futures_lite::{StreamExt};
use objc2_io_kit::{kIOHIDMaxInputReportSizeKey, kIOMasterPortDefault, kIOReturnSuccess, IOHIDDevice, IOHIDManager, IOHIDManagerOptions, IOHIDOptionsType, IOHIDReportType, IORegistryEntryIDMatching, IOReturn, IOServiceGetMatchingService};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::ptr::{null_mut, NonNull};
use std::slice::from_raw_parts;
use std::sync::LazyLock;
use objc2_core_foundation::{CFDictionary, CFIndex, CFNumber, CFRetained};

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
            self.manager.cancel();
        }
    }
}

impl IoHidManagerBackend2 {
    
}

impl Backend for IoHidManagerBackend2 {
    type Reader = IoDeviceReader;
    type Writer = DummyRW;

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
        assert!(read == true && write == false);
        unsafe {
            let service = IOServiceGetMatchingService(kIOMasterPortDefault, IORegistryEntryIDMatching(id).map(|d|d.downcast::<CFDictionary>().unwrap()));
            ensure!(service != 0, HidError::NotConnected);
            let device = IOHIDDevice::new(None, service).ok_or(HidError::message("Failed to create device"))?;
            device.set_dispatch_queue(&*DISPATCH_QUEUE);
            ensure!(device.open(0) == kIOReturnSuccess, HidError::message("Failed to open device"));

            let max_input_report_len = device
                .property(&property_key(kIOHIDMaxInputReportSizeKey))
                .ok_or(HidError::message("Failed to read input report size"))?
                .downcast_ref::<CFNumber>()
                .and_then(|n| n.as_i32())
                .unwrap() as usize;

            let mut report_buffer = ManuallyDrop::new(vec![0u8; max_input_report_len]);
            
            device.register_input_report_callback(
                NonNull::new_unchecked(report_buffer.as_mut_ptr()), 
                report_buffer.len() as CFIndex, 
                Some(hid_report_callback), 
                null_mut()
            );
            device.activate();
            
            Ok((Some(IoDeviceReader {
                device,
            }), None))
        }
        
    }


}

unsafe extern "C-unwind" fn hid_report_callback(
    _context: *mut c_void, _result: IOReturn, _sender: *mut c_void, _report_type: IOHIDReportType, _report_id: u32, report: NonNull<u8>, report_length: CFIndex
) {
    println!("REPORT: {:?}", from_raw_parts(report.as_ptr(), report_length as usize));
}

unsafe extern "C-unwind" fn added_callback(_context: *mut c_void, _result: IOReturn, _sender: *mut c_void, device: NonNull<IOHIDDevice>) {
    println!("DEVICE ADDED: {:?}", get_device_info(device.as_ref()));
}

unsafe extern "C-unwind" fn removed_callback(_context: *mut c_void, _result: IOReturn, _sender: *mut c_void, device: NonNull<IOHIDDevice>) {
    println!("DEVICE REMOVED: {:?}", get_device_info(device.as_ref()));
}

pub struct IoDeviceReader {
    device: CFRetained<IOHIDDevice>
}

impl Drop for IoDeviceReader {
    fn drop(&mut self) {
        unsafe {
            self.device.cancel();
            self.device.close(0);
        }
    }
}

unsafe impl Send for IoDeviceReader {}
unsafe impl Sync for IoDeviceReader {}

#[derive(Debug)]
pub struct DummyRW;

impl AsyncHidRead for IoDeviceReader {
    async fn read_input_report<'a>(&'a mut self, _buf: &'a mut [u8]) -> HidResult<usize> {
        futures_lite::future::pending::<()>().await;
        Ok(0)
    }
}

impl AsyncHidWrite for DummyRW {
    async fn write_output_report<'a>(&'a mut self, _buf: &'a [u8]) -> HidResult<()> {
        todo!()
    }
}
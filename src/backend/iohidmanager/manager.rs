use std::ptr::{null, null_mut};

use core_foundation::base::{kCFAllocatorDefault, CFRelease, TCFType};
use core_foundation::impl_TCFType;
use core_foundation::set::{CFSet, CFSetGetValues};
use io_kit_sys::hid::base::IOHIDDeviceRef;
use io_kit_sys::hid::keys::kIOHIDOptionsTypeNone;
use io_kit_sys::hid::manager::*;

use crate::backend::iohidmanager::device::IOHIDDevice;
use crate::{ensure, HidError, HidResult};

#[derive(Debug)]
#[repr(transparent)]
pub struct IOHIDManager(IOHIDManagerRef);

impl_TCFType!(IOHIDManager, IOHIDManagerRef, IOHIDManagerGetTypeID);

impl IOHIDManager {
    pub fn new() -> HidResult<Self> {
        let manager = unsafe { IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone) };
        ensure!(!manager.is_null(), HidError::message("Failed to create IOHIDManager"));

        unsafe { IOHIDManagerSetDeviceMatching(manager, null()) };

        Ok(unsafe { IOHIDManager::wrap_under_create_rule(manager) })
    }

    pub fn get_devices(&mut self) -> HidResult<Vec<IOHIDDevice>> {
        let devices = unsafe { IOHIDManagerCopyDevices(self.0) };
        ensure!(!devices.is_null(), HidError::message("Failed to copy device list"));
        let devices: CFSet<IOHIDDeviceRef> = unsafe { CFSet::wrap_under_create_rule(devices) };

        let num_devices = devices.len();
        let mut device_refs: Vec<IOHIDDeviceRef> = vec![null_mut(); num_devices];
        unsafe { CFSetGetValues(devices.as_concrete_TypeRef(), device_refs.as_mut_ptr() as *mut _) };

        let device_list = device_refs
            .into_iter()
            .map(IOHIDDevice::try_from)
            .filter_map(Result::ok)
            .collect();

        //unsafe { CFRelease(devices as _) };

        Ok(device_list)
    }
}

impl Drop for IOHIDManager {
    fn drop(&mut self) {
        unsafe { CFRelease(self.as_CFTypeRef()) }
    }
}

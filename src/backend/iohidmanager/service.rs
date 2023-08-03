use core_foundation::base::TCFType;
use io_kit_sys::hid::device::IOHIDDeviceGetService;
use io_kit_sys::{IOObjectRelease, IOObjectRetain, IORegistryEntryGetRegistryEntryID};
use io_kit_sys::ret::kIOReturnSuccess;
use io_kit_sys::types::io_service_t;
use mach2::port::MACH_PORT_NULL;
use crate::backend::iohidmanager::device::IOHIDDevice;
use crate::{ensure, HidError, HidResult};

#[derive(Debug)]
#[repr(transparent)]
pub struct IOService(io_service_t);

impl TryFrom<&IOHIDDevice> for IOService {
    type Error = HidError;

    fn try_from(value: &IOHIDDevice) -> Result<Self, Self::Error> {
        let service = unsafe { IOHIDDeviceGetService(value.as_concrete_TypeRef()) };
        ensure!(service != MACH_PORT_NULL, HidError::custom("Invalid IOService"));
        Ok(IOService(service))
    }
}

impl IOService {

    pub fn duplicate(&self) -> HidResult<Self> {
        let result = unsafe { IOObjectRetain(self.0) };
        ensure!(result == kIOReturnSuccess, HidError::custom("Failed to duplicate IOService"));
        Ok(IOService(self.0))
    }

    pub fn get_registry_entry_id(&self) -> HidResult<u64> {
        let copy = self.duplicate()?;
        let mut entry_id = 0;
        let result = unsafe { IORegistryEntryGetRegistryEntryID(copy.0, &mut entry_id) };
        ensure!(result == kIOReturnSuccess, HidError::custom("Failed to retrieve entry id"));
        Ok(entry_id)
    }

}

impl Drop for IOService {
    fn drop(&mut self) {
        unsafe { IOObjectRelease(self.0 as _) };
    }
}

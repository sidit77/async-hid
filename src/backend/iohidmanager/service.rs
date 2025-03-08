use core_foundation::base::TCFType;
use core_foundation::dictionary::CFMutableDictionaryRef;
use io_kit_sys::hid::device::IOHIDDeviceGetService;
use io_kit_sys::ret::kIOReturnSuccess;
use io_kit_sys::types::io_service_t;
use io_kit_sys::{
    kIOMasterPortDefault, IOObjectRelease, IOObjectRetain, IORegistryEntryGetRegistryEntryID, IORegistryEntryIDMatching, IOServiceGetMatchingService
};
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
        ensure!(service != MACH_PORT_NULL, HidError::message("Invalid IOService"));
        Ok(IOService(service))
    }
}

impl TryFrom<RegistryEntryId> for IOService {
    type Error = HidError;

    fn try_from(value: RegistryEntryId) -> Result<Self, Self::Error> {
        let service = unsafe { IOServiceGetMatchingService(kIOMasterPortDefault, value.matching()) };
        ensure!(service != MACH_PORT_NULL, HidError::message("Invalid IOService"));
        Ok(IOService(service))
    }
}

impl IOService {
    pub fn raw(&self) -> io_service_t {
        self.0
    }

    pub fn duplicate(&self) -> HidResult<Self> {
        let result = unsafe { IOObjectRetain(self.0) };
        ensure!(result == kIOReturnSuccess, HidError::message("Failed to duplicate IOService"));
        Ok(IOService(self.0))
    }

    pub fn get_registry_entry_id(&self) -> HidResult<RegistryEntryId> {
        let copy = self.duplicate()?;
        let mut entry_id = 0;
        let result = unsafe { IORegistryEntryGetRegistryEntryID(copy.0, &mut entry_id) };
        ensure!(result == kIOReturnSuccess, HidError::message("Failed to retrieve entry id"));
        Ok(RegistryEntryId(entry_id))
    }
}

impl Drop for IOService {
    fn drop(&mut self) {
        unsafe { IOObjectRelease(self.0 as _) };
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct RegistryEntryId(pub u64);

impl RegistryEntryId {
    fn matching(self) -> CFMutableDictionaryRef {
        unsafe { IORegistryEntryIDMatching(self.0) }
    }
}

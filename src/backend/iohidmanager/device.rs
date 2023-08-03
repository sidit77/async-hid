use core_foundation::base::{CFRelease, CFType, TCFType};
use core_foundation::{ConcreteCFType, impl_TCFType};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use io_kit_sys::hid::base::IOHIDDeviceRef;
use io_kit_sys::hid::device::*;
use crate::{ensure, HidError, HidResult};
use crate::backend::iohidmanager::utils::Key;

#[derive(Debug)]
#[repr(transparent)]
pub struct IOHIDDevice(IOHIDDeviceRef);

impl_TCFType!(IOHIDDevice, IOHIDDeviceRef, IOHIDDeviceGetTypeID);
unsafe impl Send for IOHIDDevice {}
unsafe impl Sync for IOHIDDevice {}

impl TryFrom<IOHIDDeviceRef> for IOHIDDevice {
    type Error = HidError;

    fn try_from(value: IOHIDDeviceRef) -> Result<Self, Self::Error> {
        ensure!(!value.is_null(), HidError::custom("IOHIDDevice is null"));
        Ok(unsafe { IOHIDDevice::wrap_under_get_rule(value) })
    }
}

impl Drop for IOHIDDevice {
    fn drop(&mut self) {
        unsafe { CFRelease(self.as_CFTypeRef()) }
    }
}

impl IOHIDDevice {

    pub fn untyped_property(&self, key: impl Key) -> HidResult<CFType> {
        let key = key.to_string();
        let property_ref = unsafe {
            IOHIDDeviceGetProperty(self.as_concrete_TypeRef(), key.as_concrete_TypeRef())
        };
        ensure!(!property_ref.is_null(), HidError::custom("Failed to retrieve property"));
        let property = unsafe { CFType::wrap_under_get_rule(property_ref) };
        Ok(property)
    }

    pub fn property<T: ConcreteCFType>(&self, key: impl Key) -> HidResult<T> {
        self.untyped_property(key)?
            .downcast_into::<T>()
            .ok_or(HidError::custom("Failed to cast property"))
    }

    pub fn get_i32_property(&self, key: impl Key) -> HidResult<i32> {
        self.property::<CFNumber>(key)
            .and_then(|v| v
                .to_i32()
                .ok_or(HidError::custom("Property is not an i32")))
    }

    pub fn get_string_property(&self, key: impl Key) -> HidResult<String> {
        self.property::<CFString>(key)
            .map(|v| v.to_string())
    }



}

use std::ffi::c_char;
use core_foundation::base::{CFRelease, CFType, kCFAllocatorDefault, TCFType};
use core_foundation::{ConcreteCFType, impl_TCFType};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringCreateWithCString, kCFStringEncodingUTF8};
use io_kit_sys::hid::base::IOHIDDeviceRef;
use io_kit_sys::hid::device::*;
use crate::{ensure, HidError, HidResult};

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

    pub fn untyped_property(&self, key: &CFString) -> HidResult<CFType> {
        let property_ref = unsafe {
            IOHIDDeviceGetProperty(self.as_concrete_TypeRef(), key.as_concrete_TypeRef())
        };
        ensure!(!property_ref.is_null(), HidError::custom("Failed to retrieve property"));
        let property = unsafe { CFType::wrap_under_get_rule(property_ref) };
        Ok(property)
    }

    pub fn property<T: ConcreteCFType>(&self, key: &CFString) -> HidResult<T> {
        self.untyped_property(key)?
            .downcast_into::<T>()
            .ok_or(HidError::custom("Failed to cast property"))
    }

    pub fn get_i32_property(&self, key: *const c_char) -> HidResult<i32> {
        self.property::<CFNumber>(&make_string(key))
            .and_then(|v| v
                .to_i32()
                .ok_or(HidError::custom("Property is not an i32")))
    }

    pub fn get_string_property(&self, key: *const c_char) -> HidResult<String> {
        self.property::<CFString>(&make_string(key))
            .map(|v| v.to_string())
    }



}

fn make_string(string: *const c_char) -> CFString {
    unsafe {
        let string = CFStringCreateWithCString(kCFAllocatorDefault, string, kCFStringEncodingUTF8);
        CFString::wrap_under_create_rule(string)
    }
}
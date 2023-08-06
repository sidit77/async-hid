use std::ffi::c_char;

use core_foundation::base::{kCFAllocatorDefault, CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{kCFStringEncodingUTF8, CFString, CFStringCreateWithCString};
use core_foundation::ConcreteCFType;

use crate::{HidError, HidResult};

pub trait Key {
    fn to_string(self) -> CFString;
}

impl Key for *const c_char {
    fn to_string(self) -> CFString {
        unsafe {
            let string = CFStringCreateWithCString(kCFAllocatorDefault, self, kCFStringEncodingUTF8);
            CFString::wrap_under_create_rule(string)
        }
    }
}

pub trait CFDictionaryExt {
    fn lookup_untyped(&self, key: impl Key) -> HidResult<CFType>;

    fn lookup<T: ConcreteCFType>(&self, key: impl Key) -> HidResult<T> {
        self.lookup_untyped(key)?
            .downcast_into::<T>()
            .ok_or(HidError::custom("Failed to cast value"))
    }

    fn lookup_i32(&self, key: impl Key) -> HidResult<i32> {
        self.lookup::<CFNumber>(key)
            .and_then(|v| v.to_i32().ok_or(HidError::custom("Value is not an i32")))
    }
}

impl CFDictionaryExt for CFDictionary<CFString> {
    fn lookup_untyped(&self, key: impl Key) -> HidResult<CFType> {
        let item_ref = self
            .find(key.to_string())
            .ok_or(HidError::custom("Couldn't find value in dict"))?;
        Ok(unsafe { CFType::wrap_under_get_rule(*item_ref) })
    }
}

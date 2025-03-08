#![doc = include_str!("../README.md")]

mod backend;
mod error;
mod device_info;
mod device;
mod traits;

use static_assertions::assert_impl_all;

pub use crate::error::{HidError, HidResult};
pub use device::{DeviceReaderWriter, DeviceReader, DeviceWriter};
pub use device_info::{DeviceInfo, HidBackend, Device, DeviceId};
pub use traits::{AsyncHidRead, AsyncHidWrite};
pub use backend::{BackendType};



assert_impl_all!(DeviceReaderWriter: Send, Sync);
assert_impl_all!(DeviceInfo: Send, Sync);


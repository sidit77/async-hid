#![doc = include_str!("../README.md")]

mod backend;
mod error;
mod device_info;
mod device_id;
mod device;

use static_assertions::assert_impl_all;

pub use crate::error::{ErrorSource, HidError, HidResult};
pub use device::Device;
pub use device_id::DeviceId;
pub use device_info::{AccessMode, DeviceInfo, SerialNumberExt};



assert_impl_all!(Device: Send, Sync);
assert_impl_all!(DeviceInfo: Send, Sync);
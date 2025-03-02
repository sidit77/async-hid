#![doc = include_str!("../README.md")]

mod backend;
mod error;
mod device_info;
mod device;
mod traits;

use static_assertions::assert_impl_all;

pub use crate::error::{HidError, HidResult};
pub use device::{Device, DeviceReader, DeviceWriter};
pub use device_info::{DeviceInfo};
pub use traits::{AsyncHidRead, AsyncHidWrite};
pub use backend::{Backend};



assert_impl_all!(Device: Send, Sync);
assert_impl_all!(DeviceInfo: Send, Sync);
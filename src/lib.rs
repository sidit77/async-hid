#![doc = include_str!("../README.md")]

mod backend;
mod error;
mod device_info;
mod device;
mod traits;
mod utils;

use static_assertions::assert_impl_all;

pub use crate::error::{HidError, HidResult};
pub use device::{DeviceReader, DeviceReaderWriter, DeviceWriter};
pub use device_info::{Device, DeviceId, DeviceInfo, HidBackend};
pub use traits::{AsyncHidRead, AsyncHidWrite};
pub use backend::BackendType;



assert_impl_all!(DeviceReaderWriter: Send, Sync);
assert_impl_all!(DeviceInfo: Send, Sync);


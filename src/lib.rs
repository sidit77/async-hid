#![doc = include_str!("../README.md")]

mod backend;
mod device;
mod device_info;
mod error;
mod traits;
mod utils;

/// All available backends for the current platform
pub use backend::BackendType;
pub use device::{DeviceReader, DeviceReaderWriter, DeviceWriter};
pub use device_info::{Device, DeviceId, DeviceInfo, HidBackend, DeviceEvent};
use static_assertions::assert_impl_all;
pub use traits::{AsyncHidRead, AsyncHidWrite};

pub use crate::error::{HidError, HidResult};

assert_impl_all!(DeviceReaderWriter: Send, Sync);
assert_impl_all!(DeviceInfo: Send, Sync);

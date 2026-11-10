//! Tests of the device handle that need no HID device.

use std::fs::{remove_file, File};

use windows::core::{HSTRING, PCWSTR};

use super::device::Device;

/// A plain file has no product string, like a HID device that does not
/// provide one.
#[test]
fn a_missing_product_string_is_an_empty_name() {
    let path = std::env::temp_dir().join(format!("async-hid-no-product-string-{}", std::process::id()));
    File::create(&path).expect("Failed to create the test file");
    let wide = HSTRING::from(path.as_path());
    let device = Device::open(PCWSTR(wide.as_ptr()), false, false, None).expect("Failed to open the test file");

    let name = device.name();

    drop(device);
    let _ = remove_file(&path);
    assert_eq!(name.expect("A missing product string must not be an error"), "");
}

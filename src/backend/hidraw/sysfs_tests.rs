//! Tests of the sysfs side of the backend against a fake sysfs tree.

use std::fs::{create_dir_all, remove_dir_all, write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use super::enumerate_class_dir;

/// A temporary directory that is removed again when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("async-hid-{name}-{}", std::process::id()));
        let _ = remove_dir_all(&path);
        create_dir_all(&path).expect("Failed to create the test directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = remove_dir_all(&self.0);
    }
}

/// Creates the sysfs directory of a hidraw node: `<node>/device/uevent` and
/// `<node>/device/report_descriptor`.
fn fake_hidraw_node(node: &Path, descriptor: &[u8]) {
    let device = node.join("device");
    create_dir_all(&device).expect("Failed to create the device directory");
    write(
        device.join("uevent"),
        "DRIVER=hid-generic\nHID_ID=0003:0000046D:0000C52B\nHID_NAME=Test Device\nHID_UNIQ=\n"
    )
    .expect("Failed to write uevent");
    write(device.join("report_descriptor"), descriptor).expect("Failed to write the report descriptor");
}

/// Usage Page (Generic Desktop), Usage (Mouse), Collection (Application), End Collection
const MOUSE: &[u8] = &[0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0xc0];

#[test]
fn a_device_removed_during_enumeration_does_not_fail_the_others() {
    let root = TempDir::new("enumerate-removed");
    let class = root.0.join("class");
    create_dir_all(&class).expect("Failed to create the class directory");

    let node = root.0.join("devices/hidraw0");
    fake_hidraw_node(&node, MOUSE);
    symlink(&node, class.join("hidraw0")).expect("Failed to link hidraw0");
    // The link of a node whose device is already gone.
    symlink(root.0.join("devices/hidraw1"), class.join("hidraw1")).expect("Failed to link hidraw1");

    let results = enumerate_class_dir(&class)
        .expect("The enumeration itself must succeed")
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1, "the removed device is one error item");
    let devices = results
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert_eq!(devices.len(), 1);
    assert_eq!((devices[0].vendor_id, devices[0].product_id), (0x046d, 0xc52b));
    assert_eq!((devices[0].usage_page, devices[0].usage_id), (0x01, 0x02));
}

//! Tries to open every enumerated device once, prints the success/failure
//! counts, then parks so the process can be inspected from outside (e.g.
//! `leaks <pid>` on macOS) to check that neither successful nor failed opens
//! leak per-device resources.

use async_hid::{HidBackend, HidResult};
use futures_lite::stream::StreamExt;

#[pollster::main]
async fn main() -> HidResult<()> {
    let devices: Vec<_> = HidBackend::default().enumerate().await?.collect().await;
    let (mut opened, mut failed) = (0u32, 0u32);
    for device in &devices {
        match device.open().await {
            Ok(_handle) => opened += 1,
            Err(_) => failed += 1,
        }
    }
    println!("pid={} devices={} opened={} failed={}", std::process::id(), devices.len(), opened, failed);
    std::thread::sleep(std::time::Duration::from_secs(300));
    Ok(())
}

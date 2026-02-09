use async_hid::{AsyncHidFeatureHandle, HidBackend, HidResult};
use futures_lite::StreamExt;
use simple_logger::SimpleLogger;

const WOOTING_VID: u16 = 0x31E3;
const WOOTING_USAGE_PAGE: u16 = 0xFF55;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let backend = HidBackend::default();

    let device = backend
        .enumerate()
        .await?
        .find(|info| info.vendor_id == WOOTING_VID && info.usage_page == WOOTING_USAGE_PAGE)
        .await
        .expect("Could not find Wooting device");

    println!(
        "Found: {} (VID: 0x{:04X}, PID: 0x{:04X})",
        device.name, device.vendor_id, device.product_id
    );

    let mut handle = device.open_feature_handle().await?;

    let report = [
        0x01, //Report ID
        0xd1, 0xda, //wooting magic number
        0x17, //command: activate profile
        0x01, 0x00, 0x00, 0x00, //u32 parameter: profile index (0-based)
    ];
    handle.write_feature_report(&report).await?;

    println!("Switched to profile 2");

    Ok(())
}

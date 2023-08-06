use async_hid::{DeviceInfo, HidResult};
use simple_logger::SimpleLogger;

#[tokio::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    for device in DeviceInfo::enumerate().await? {
        println!(
            "{}: 0x{:X} 0x{:X} 0x{:X} 0x{:X}",
            device.name, device.usage_page, device.usage_id, device.vendor_id, device.product_id
        );
    }
    Ok(())
}

use async_hid::{DeviceInfo, HidResult};
use simple_logger::SimpleLogger;
use futures_lite::stream::StreamExt;

#[tokio::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    DeviceInfo::enumerate()
        .await?
        .for_each(|device| {
            println!(
                "{}: 0x{:X} 0x{:X} 0x{:X} 0x{:X}",
                device.name, device.usage_page, device.usage_id, device.vendor_id, device.product_id
            );
        })
        .await;
    Ok(())
}

use async_hid::{DeviceInfo, HidResult, SerialNumberExt};
use futures_lite::stream::StreamExt;
use log::LevelFilter;
use simple_logger::SimpleLogger;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Trace)
        .init()
        .unwrap();

    DeviceInfo::enumerate()
        .await?
        .for_each(|device| {
            println!(
                "{}: 0x{:X} 0x{:X} 0x{:X} 0x{:X} {:?}",
                device.name,
                device.usage_page,
                device.usage_id,
                device.vendor_id,
                device.product_id,
                device.serial_number()
            );
        })
        .await;
    Ok(())
}

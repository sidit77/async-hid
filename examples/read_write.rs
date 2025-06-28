use async_hid::{AsyncHidRead, AsyncHidWrite, Device, HidBackend, HidError, HidResult};
use futures_lite::StreamExt;
use simple_logger::SimpleLogger;

use async_hid::HidOperations;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let mut device = HidBackend::default()
        .enumerate()
        .await?
        .find(|info: &Device| info.matches(0xFFC0, 0x1, 0x1038, 0x2206))
        .await
        .inspect(|info| {
            println!(
                "{}: 0x{:X} 0x{:X} 0x{:X} 0x{:X} {:?}",
                info.name, info.usage_page, info.usage_id, info.vendor_id, info.product_id, info.id
            );
        })
        .expect("Could not find device")
        .open()
        .await?;

    let report = device.get_input_report();
    if let Ok(report) = report {
        println!("Input Report ({}): {report:?}", report.len());
    } else {
        println!("Failed to get input report: {:?}", report);
    }
    device.write_output_report(&[0x0, 0xb0]).await?;
    let mut buffer = [0u8; 8];
    let size = device.read_input_report(&mut buffer).await?;
    println!("{:?}", &buffer[..size]);
    Ok::<(), HidError>(())
}

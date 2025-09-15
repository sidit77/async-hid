use std::time::Duration;

use async_hid::{HidBackend, HidError, HidResult};
use async_io::Timer;
use futures_lite::{FutureExt, StreamExt};
use simple_logger::SimpleLogger;

const SONY_VID: u16 = 0x054C;
const DUALSENSE_PID: u16 = 0x0CE6;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let device = HidBackend::default()
        .enumerate()
        .await?
        .find(|info| info.vendor_id == SONY_VID && info.product_id == DUALSENSE_PID)
        .await
        .expect("Could not find device");

    let mut buffer = [0u8; 41];
    buffer[0] = 0x05; // Calibration data report ID
    let size = device
        .read_feature_report(&mut buffer)
        .or(async {
            Timer::after(Duration::from_secs(1)).await;
            Err(HidError::Message("Timeout reading feature report".into()))
        })
        .await?;

    println!("Read {} bytes: {:02X?}", size, &buffer[..size]);

    Ok(())
}

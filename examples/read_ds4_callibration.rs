use std::time::Duration;

use async_hid::{HidBackend, HidError, HidResult};
use async_io::Timer;
use futures_lite::{FutureExt, StreamExt};
use simple_logger::SimpleLogger;

const SONY_VID: u16 = 0x054C;
const DUAL_SHOCK_4_PID: u16 = 0x05C4;

const CALIBRATION_FLAGS_REPORT_ID: u8 = 0x10;

const CALIBRATION_FLAGS_STICK_MIX_MAX: u32 = 1 << 8;
const CALIBRATION_FLAGS_STICK_CENTER: u32 = 1 << 9;
const CALIBRATION_FLAGS_L2: u32 = 1 << 10;
const CALIBRATION_FLAGS_R2: u32 = 1 << 11;
const CALIBRATION_FLAGS_GYROSCOPE: u32 = 1 << 24;
const CALIBRATION_FLAGS_ACCELEROMETER: u32 = 1 << 25;


#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let device = HidBackend::default()
        .enumerate()
        .await?
        .find(|info| info.vendor_id == SONY_VID && info.product_id == DUAL_SHOCK_4_PID)
        .await
        .expect("Could not find device");

    let mut buffer = [0u8; 5];
    buffer[0] = CALIBRATION_FLAGS_REPORT_ID;
    let size = device
        .read_feature_report(&mut buffer)
        .or(async {
            Timer::after(Duration::from_secs(1)).await;
            Err(HidError::Message("Timeout reading feature report".into()))
        })
        .await?;

    let flags = u32::from_be_bytes(buffer[1..size].try_into().unwrap());

    println!("Calibration flags: {flags:032b}");
    println!("    Stick mix max: {}", flags & CALIBRATION_FLAGS_STICK_MIX_MAX != 0);
    println!("    Stick center: {}", flags & CALIBRATION_FLAGS_STICK_CENTER != 0);
    println!("    L2: {}", flags & CALIBRATION_FLAGS_L2 != 0);
    println!("    R2: {}", flags & CALIBRATION_FLAGS_R2 != 0);
    println!("    Gyroscope: {}", flags & CALIBRATION_FLAGS_GYROSCOPE != 0);
    println!("    Accelerometer: {}", flags & CALIBRATION_FLAGS_ACCELEROMETER != 0);


    Ok(())
}

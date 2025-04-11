use std::time::Duration;

use async_hid::{AsyncHidRead, HidBackend, HidResult};
use async_io::Timer;
use futures_lite::stream::StreamExt;
use simple_logger::SimpleLogger;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let mut device = HidBackend::default()
        .enumerate()
        .await?
        .find(|info| info.matches(0x1, 0x1, 0x62A, 0x5918))
        .await
        //.find(|info| info.matches(0xFF00, 0x1, 0x1038, 0x2206))
        .expect("Could not find device")
        .open_readable()
        .await?;
    println!("Waiting");
    Timer::after(Duration::from_millis(3000)).await;
    println!("Reading");
    let mut buffer = [0u8; 8];
    loop {
        let size = device.read_input_report(&mut buffer).await?;
        println!("{:?}", &buffer[..size])
    }
}

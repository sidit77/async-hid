use async_hid::{AccessMode, DeviceInfo, HidResult};
use futures_lite::stream::StreamExt;
use simple_logger::SimpleLogger;

#[tokio::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let device = DeviceInfo::enumerate()
        .await?
        .find(|info: &DeviceInfo| info.matches(0x1, 0x1, 0x46D, 0xC016))
        .await
        //.find(|info| info.matches(0xFF00, 0x1, 0x1038, 0x2206))
        .expect("Could not find device")
        .open(AccessMode::Read)
        .await?;

    let mut buffer = [0u8; 8];
    loop {
        let size = device.read_input_report(&mut buffer).await?;
        println!("{:?}", &buffer[..size])
    }
}

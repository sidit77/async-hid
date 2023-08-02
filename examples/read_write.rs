use async_hid::{DeviceInfo, HidResult};

#[tokio::main]
async fn main() -> HidResult<()> {
    let device = DeviceInfo::enumerate()
        .await?
        .iter()
        .find(|info| info.matches(0xFFC0, 0x1, 0x1038, 0x2206))
        .expect("Could not find device")
        .open()
        .await?;

    device.write_output_report(&[0x0, 0xb0]).await?;
    let mut buffer = [0u8; 8];
    let size = device.read_input_report(&mut buffer).await?;
    println!("{:?}", &buffer[..size]);
    Ok(())
}

use async_hid::{DeviceInfo, HidResult};

#[pollster::main]
async fn main() -> HidResult<()> {
    for device in DeviceInfo::enumerate().await? {
        println!("{:#?}", device);
    }
    Ok(())
}

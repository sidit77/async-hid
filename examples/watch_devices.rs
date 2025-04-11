use async_hid::{HidBackend, HidResult};
use futures_lite::StreamExt;
use simple_logger::SimpleLogger;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    HidBackend::default()
        .watch()?
        .for_each(|event| println!("{:?}", event))
        .await;
   
    Ok(())
}

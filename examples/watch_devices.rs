use async_io::Timer;
use async_hid::{HidBackend, HidResult};
use futures_lite::{FutureExt, StreamExt};
use simple_logger::SimpleLogger;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    HidBackend::default()
        .watch()?
        .for_each(|event| println!("{:?}", event))
        .race(async { Timer::after(std::time::Duration::from_secs(10)).await; })
        .await;
   
    Ok(())
}

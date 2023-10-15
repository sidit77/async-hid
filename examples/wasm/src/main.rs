use async_hid::DeviceInfo;
use futures_lite::stream::StreamExt;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn start() {
    log::info!("Starting enumeration");

    DeviceInfo::enumerate()
        .await
        .unwrap()
        .for_each(|device| {
            log::info!(
                "{}: 0x{:X} 0x{:X}",
                device.name,
                device.vendor_id,
                device.product_id
            );
        })
        .await;
}

#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Trace).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    simple_logger::SimpleLogger::new().init().unwrap();
    pollster::block_on(start());
}

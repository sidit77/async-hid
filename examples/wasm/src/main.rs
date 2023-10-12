async fn start() {
    log::info!("Hello World");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Trace).unwrap();
    wasm_bindgen_futures::spawn_local(start());
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    simple_logger::SimpleLogger::new().init().unwrap();
    pollster::block_on(start());
}

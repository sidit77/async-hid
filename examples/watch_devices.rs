use std::collections::HashSet;

use async_hid::{Device, DeviceEvent, DeviceInfo, HidBackend, HidResult};
use futures_lite::StreamExt;
use simple_logger::SimpleLogger;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let backend = HidBackend::default();

    let mut watcher = backend.watch()?;
    let mut device_set = backend
        .enumerate()
        .await?
        .map(Device::to_device_info)
        .collect::<HashSet<_>>()
        .await;

    loop {
        //print_device_set(&device_set);
        println!("Number of connected devices: {}", device_set.len());
        if let Some(event) = watcher.next().await {
            //println!("Event: {:?}", event);
            match event {
                DeviceEvent::Connected(id) => device_set.extend(
                    backend
                        .query_devices(&id)
                        .await?
                        .map(Device::to_device_info)
                ),
                DeviceEvent::Disconnected(id) => device_set.retain(|device| device.id != id)
            }
        }
    }
}

#[allow(dead_code)]
fn print_device_set(device_set: &HashSet<DeviceInfo>) {
    println!("Connected devices:");
    for device in device_set {
        println!("  {}", if device.name.is_empty() { "(unnamed)" } else { &device.name });
        println!("    id: {:?}", device.id);
        println!(
            "    vid/pid/usage/page: 0x{:X} 0x{:X} 0x{:X} 0x{:X}",
            device.vendor_id, device.product_id, device.usage_id, device.usage_page
        );
        if let Some(serial) = &device.serial_number {
            println!("    serial number: {:?}", serial)
        }
    }
    println!("total: {}", device_set.len());
}

# async-hid
A Rust library for asynchronously interacting with HID devices. 

This crate aims the be a replacement for [hidapi-rs](https://github.com/ruabmbua/hidapi-rs) without the baggage that comes from being a wrapper around a C library.

This crate generally offers a simpler and more streamlined api while also supporting async as best as possible. 

## Example

```rust
use async_hid::{AccessMode, DeviceInfo, HidResult};
use simple_logger::SimpleLogger;
use futures_lite::StreamExt;

#[tokio::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let device = DeviceInfo::enumerate()
        .await?
        //Steelseries Arctis Nova 7X headset
        .find(|info: &DeviceInfo | info.matches(0xFFC0, 0x1, 0x1038, 0x2206))
        .await
        .expect("Could not find device")
        .open(AccessMode::ReadWrite)
        .await?;

    device.write_output_report(&[0x0, 0xb0]).await?;
    let mut buffer = [0u8; 8];
    let size = device.read_input_report(&mut buffer).await?;
    println!("{:?}", &buffer[..size]);
    Ok(())
}
```


## Platform Support

| Operating System | Underlying API                                 |
|------------------|------------------------------------------------|
| Windows          | WinRT (`Windows.Devices.HumanInterfaceDevice`) |
| Linux            | hidraw                                         |
| MacOs            | IOHIDManager                                   |


## Async
The amount of asynchronicity that each OS provides varies. The following tables gives a rough overview which calls utilize async under the hood.

|         | `enumerate` | `open` | `read_input_report` | `write_output_report` |
|---------|-------------|--------|---------------------|-----------------------|
| Windows | ✔️          | ✔️     | ✔️                  | ✔️                    |
| Linux   | ❌           | ❌      | ✔️                  | ✔️                    |
| MacOS   | ❌           | ✔️     | ✔️                  | ❌                     |

Under Linux this crate requires a `tokio` runtime while the Window and MacOS backends are runtime agnostic.

## Planned Features
- [ ] Reading / Writing feature reports
- [ ] Listening for changes to the device list
- [ ] More unified error handling

## License
MIT License
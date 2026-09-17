# async-hid
A Rust library for asynchronously interacting with HID devices. 

This crate aims to be a replacement for [hidapi-rs](https://github.com/ruabmbua/hidapi-rs) without the baggage that comes from being a wrapper around a C library.

This crate generally offers a simpler and more streamlined api while also supporting async as best as possible. 

## Example

```rust
use async_hid::{AccessMode, DeviceInfo, HidResult};
use simple_logger::SimpleLogger;
use futures_lite::StreamExt;

#[pollster::main]
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
| Windows          | Win32 (`Windows.Win32.Devices`)                |
| Windows          | WinRT (`Windows.Devices.HumanInterfaceDevice`) |
| Linux            | hidraw                                         |
| FreeBSD          | hidraw                                         |
| MacOs            | IOHIDManager                                   |

Under Windows this crate uses either `win32` (default) or `winrt` feature for backend.


## Async
The amount of asynchronicity that each OS provides varies. The following table gives a rough overview which calls utilize async under the hood.

|                 | `enumerate`  | `open` | `read_input_report` | `write_output_report` |
|-----------------|--------------|--------|---------------------|-----------------------|
| Windows (Win32) | ❌️           | ️️ ❌️    | ✔️                  | ✔️                    |
| Windows (WinRT) | ✔️           | ✔️     | ✔️                  | ✔️                    |
| Linux           | ❌            | ❌      | ✔️                  | ✔️                    |
| FreeBSD         | ❌            | ❌      | ✔️                  | ✔️                    |
| MacOS           | ❌            | ✔️     | ✔️                  | ✔️                     |

Under Linux this crate uses either `async-io` (default) or `tokio` feature for the async functionality.

Under FreeBSD this crate likewise uses either `async-io` (default) or `tokio`. Note that FreeBSD's `hidraw(4)`
implements only the `EVFILT_READ` kqueue filter and rejects `EVFILT_WRITE`, so the tokio backend registers
descriptors with read interest only and performs writes directly. This is sound because writes to a hidraw node
never block: `hidraw_write()` calls `hid_write()` synchronously and `hidraw_poll()` already reports `POLLOUT`
unconditionally.

FreeBSD additionally requires the `hidraw` kernel module (`hidraw_load="YES"` in `loader.conf`),
`hw.usb.usbhid.enable=1`, and read access to `/dev/hidraw*` (`0600 root:operator` by default, so add a
`devfs.rules(5)` entry or join the `operator` group).

## Planned Features
- Redesign how feature reads and write are integrated exposed.
  - Allow feature reads for read-only devices?
  - Allow opening read/write/feature handles all at once?
- Add support for WASM via WebHID


## License
MIT License
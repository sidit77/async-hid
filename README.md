# async-hid
A Rust library for asynchronously interacting with HID devices. 

This crate aims the be a replacement for [hidapi-rs](https://github.com/ruabmbua/hidapi-rs) without the baggage that comes from being a wrapper around a C library.

This crate generally offers a simpler and more streamlined api while also supporting async as best as possible. 

## Example

```rust
#[tokio::main]
async fn main() -> HidResult<()> {
    let device = DeviceInfo::enumerate()
        .await?
        .iter()
        //Optical Logitech Mouse
        .find(|info| info.matches(0x1, 0x1, 0x46D, 0xC016))
        .expect("Could not find device")
        .open(AccessMode::Read)
        .await?;

    let mut buffer = [0u8; 8];
    loop {
        let size = device.read_input_report(&mut buffer).await?;
        println!("{:?}", &buffer[..size])
    }
}

```


## Platform Support

| Operating System | Underlying API                                 |
|------------------|------------------------------------------------|
| Windows          | WinRT (`Windows.Devices.HumanInterfaceDevice`) |
| Linux            | udev + hidraw                                  |
| MacOs            | IOHIDManager                                   |

Compiling under Linux requires the dev package of `udev`.
```shell
sudo apt install libudev-dev
```

## Async
The amount of asynchronicity that each OS provides varies. The following tables gives a rough overview which calls utilize async under the hood.

|         | `enumerate` | `open` | `read_input_report` | `write_output_report` |
|---------|-------------|--------|---------------------|-----------------------|
| Windows | ✔️          | ✔️     | ✔️                  | ✔️                    |
| Linux   | ✔️*         | ❌      | ✔️                  | ✔️                    |
| MacOS   | ❌           | ✔️     | ✔️                  |                       |

(*) using  the `tokio` thread pool

Under Linux this crate requires a `tokio` runtime while the Window and MacOS backends are runtime agnostic.


## Planned Features
- [ ] Reading / Writing feature reports
- [ ] Listening for changes to the device list
- [ ] More unified error handling

## License
MIT License
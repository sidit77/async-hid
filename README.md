# async-hid
A Rust library for asynchronously interacting with HID devices.

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


## Planned Features
- [ ] Reading / Writing feature reports
- [ ] Listening for changes to the deivice list

## License
MIT License
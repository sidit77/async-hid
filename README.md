# async-hid
A Rust library for asynchronously interacting with HID devices. 

This crate aims to be a replacement for [hidapi-rs](https://github.com/ruabmbua/hidapi-rs) without the baggage that comes from being a wrapper around a C library.

This crate generally offers a simpler and more streamlined api while also supporting async as best as possible. 

## Example

```rust,no_run
use async_hid::{AsyncHidRead, AsyncHidWrite, Device, HidBackend, HidResult};
use futures_lite::StreamExt;
use simple_logger::SimpleLogger;

#[pollster::main]
async fn main() -> HidResult<()> {
    SimpleLogger::new().init().unwrap();

    let mut device = HidBackend::default()
        .enumerate()
        .await?
        //Steelseries Arctis Nova 7X headset
        .find(|info: &Device| info.matches(0xFFC0, 0x1, 0x1038, 0x2206))
        .await
        .expect("Could not find device")
        .open()
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

## Shared and Exclusive Access
`Device::open`, `open_readable`, `open_writeable` and `open_feature_handle` open a device with the platform
default access. `Device::open_options()` returns an `OpenOptions` builder that requests an explicit `Access`
mode and either provides that mode, as defined per backend below, or fails with `HidError::Unsupported` or
`HidError::Busy`.

| Backend         | Platform default              | `Access::Shared` | `Access::Exclusive`                        |
|-----------------|-------------------------------|------------------|--------------------------------------------|
| Windows (Win32) | shared                        | supported        | supported (share mode `0`)                 |
| Windows (WinRT) | not specified by WinRT        | as the default   | `Unsupported`                              |
| Linux           | shared                        | supported        | `Unsupported` (hidraw has none)            |
| FreeBSD         | exclusive (one open per node) | `Unsupported`    | supported                                  |
| macOS           | shared                        | supported        | supported (`kIOHIDOptionsTypeSeizeDevice`) |

`Access::Exclusive` means that no other handle can open the device for input or output report access while
the exclusive handle is held. What that covers differs per backend:

- macOS: the device is seized at the IOHIDLib level. Measured on 2026-10-11 with a YKUSH3 on macOS on Apple
  silicon; each statement names its scope:
  - while the seize is held, exclusive, shared and default opens fail with `Busy` and the seizing handle
    keeps working (within one process and across two processes);
  - while the seize is held, the feature handle that `read_feature_report` opens fails with `Busy`
    (within one process only);
  - closing the seizing handle releases the seize (within one process and across two processes);
  - a seize over an existing shared or default handle succeeds. That handle's writes then fail with `Busy`
    (`kIOReturnExclusiveAccess`, `0xE00002C5`) instead of hanging, and once the seize is released it works
    again without being reopened (within one process and across two processes). So `Access::Shared` does
    not protect against a later exclusive open;
  - two shared handles coexist (within one process and across two processes);
  - every input report is delivered to every open shared or default handle, so a handle also receives the
    answers to other handles' requests and must match each answer to its own request (within one process
    and across two processes);
  - not measured: input report reads on a shared or default handle during another handle's seize; only
    the writes were measured.
- Windows (Win32): other opens that request read or write access fail with `Busy`. Opens that request
  neither are not checked by Windows, so enumeration, feature handles with the default or shared access and
  `Device::read_feature_report` still work and can reach feature reports. An exclusive feature handle is
  opened with read and write access, which may fail with access denied on keyboard and mouse collections.
  Unverified on a Windows machine.
- FreeBSD: any other open of the same hidraw node fails with `Busy`, including enumeration, `query_devices`
  and `read_feature_report` of this crate. Other drivers using the device are not covered.

See the documentation of `Access` for the details per backend.

The macOS measurement uses a Yepkit YKUSH3 and only ever reads the state of port 1. It can be repeated:
within one process with the ignored test `cargo test --test access -- --ignored --nocapture`, across
processes with the `examples/access_probe.rs` example, run in two terminals:

```text
# terminal 1: hold the device and poll its status every 500 ms
cargo run --example access_probe -- hold shared 10
# terminal 2, while terminal 1 holds it: open once and read once
cargo run --example access_probe -- probe exclusive
```

Each of `hold` and `probe` takes `shared`, `exclusive` or `default`. To see how a holder fares during
another process's seize, run `hold shared 10` in terminal 1 and `hold exclusive 2` in terminal 2.

## Planned Features
- Redesign how feature reads and write are integrated exposed.
  - Allow feature reads for read-only devices?
  - Allow opening read/write/feature handles all at once?
- Add support for WASM via WebHID


## License
MIT License
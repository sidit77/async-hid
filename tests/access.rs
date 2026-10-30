//! Hardware checks for `OpenOptions::access`.
//!
//! They need a Yepkit YKUSH3 (vendor 0x04d8, product 0xf11b) attached and are
//! ignored by default:
//!
//! ```text
//! cargo test --test access -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The board is only ever asked for the state of downstream ports 1 and 2
//! (opcodes 0x21 and 0x22). No switching command is sent. The opcodes and the
//! answer layout are taken from the protocol table in the ykush3-rs README,
//! section "USB control protocol": 0x21 to 0x23 ask for the state of port 1
//! to 3, byte 0 of the 64 byte answer is 0x01 on success, byte 1 carries the
//! on/off state in the high nibble and the port number in the low nibble.
//!
//! An answer counts only if its port number matches the request, so a stale
//! answer to an earlier request can not pass for a fresh one. Answers for
//! another port are skipped and counted, and the count is printed.
//!
//! Every write and read is bounded by a timeout, so a handle that has been cut
//! off makes a test fail or report instead of hang.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::future::Future;
use std::time::{Duration, Instant};

use async_hid::{Access, AsyncHidRead, AsyncHidWrite, Device, DeviceReaderWriter, HidBackend, HidError, HidResult};
use async_io::Timer;
use futures_lite::{future, StreamExt};

const YKUSH3_VID: u16 = 0x04d8;
const YKUSH3_PID: u16 = 0xf11b;
const REPORT_SIZE: usize = 64;
/// Opcode base of the port state requests: 0x20 | port.
const PORT_STATUS: u8 = 0x20;
const ACK: u8 = 0x01;
const TIMEOUT: Duration = Duration::from_secs(2);

async fn ykush3() -> Device {
    HidBackend::default()
        .enumerate()
        .await
        .expect("enumeration failed")
        .find(|d: &Device| d.vendor_id == YKUSH3_VID && d.product_id == YKUSH3_PID)
        .await
        .expect("no YKUSH3 attached")
}

/// Runs `op`, giving up after [TIMEOUT].
async fn timed<T>(op: impl Future<Output = T>) -> Option<T> {
    timed_for(TIMEOUT, op).await
}

/// Runs `op`, giving up after `limit`.
async fn timed_for<T>(limit: Duration, op: impl Future<Output = T>) -> Option<T> {
    future::or(async { Some(op.await) }, async {
        Timer::after(limit).await;
        None
    })
    .await
}

/// Asks for the state of downstream `port` (1 to 3). Describes every way
/// this can go wrong instead of panicking, so a caller can record the
/// outcome. Returns the state byte and the number of skipped answers that
/// named another port.
async fn try_port_status(handle: &mut DeviceReaderWriter, port: u8) -> Result<(u8, usize), String> {
    // Unnumbered reports: a leading zero report id, then the 64 byte report.
    let mut request = [0u8; REPORT_SIZE + 1];
    request[1] = PORT_STATUS | port;
    match timed(handle.write_output_report(&request)).await {
        None => return Err(format!("write timed out after {:?}", TIMEOUT)),
        Some(Err(err)) => return Err(format!("write failed: {:?}", err)),
        Some(Ok(())) => {}
    }

    let deadline = Instant::now() + TIMEOUT;
    let mut skipped = 0;
    loop {
        let mut answer = [0u8; REPORT_SIZE];
        let remaining = deadline.saturating_duration_since(Instant::now());
        let read = match timed_for(remaining, handle.read_input_report(&mut answer)).await {
            None => return Err(format!("no answer for port {port} within {:?} ({skipped} skipped)", TIMEOUT)),
            Some(Err(err)) => return Err(format!("read failed: {:?}", err)),
            Some(Ok(read)) => read
        };

        if read != REPORT_SIZE || answer[0] != ACK {
            return Err(format!("unexpected answer: {read} bytes, {:02x?}", &answer[..2]));
        }
        if answer[1] & 0x0f == port {
            return Ok((answer[1], skipped));
        }
        skipped += 1;
    }
}

async fn port_status(handle: &mut DeviceReaderWriter, which: &str, port: u8) -> u8 {
    match try_port_status(handle, port).await {
        Ok((state, skipped)) => {
            if skipped > 0 {
                println!("status read of port {port} through the {which} handle skipped {skipped} answer(s) for another port");
            }
            state
        }
        Err(err) => panic!("status read of port {port} through the {which} handle: {err}")
    }
}

fn describe<T>(result: &HidResult<T>) -> String {
    match result {
        Ok(_) => "Ok".to_string(),
        Err(err) => format!("Err({:?})", err)
    }
}

async fn open(device: &Device, access: Access) -> HidResult<DeviceReaderWriter> {
    timed(device.open_options().access(access).open())
        .await
        .expect("open timed out")
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs a YKUSH3 board"]
fn linux_shared_opens_and_exclusive_is_unsupported() {
    pollster::block_on(async {
        let device = ykush3().await;

        let mut shared = open(&device, Access::Shared).await.expect("shared open");
        let state = port_status(&mut shared, "shared", 1).await;
        println!("shared open: Ok, port 1 state 0x{state:02x}");

        // A second shared handle is allowed by hidraw.
        let second = open(&device, Access::Shared).await;
        println!("second shared open while the first is held: {}", describe(&second));
        assert!(second.is_ok());

        let exclusive = open(&device, Access::Exclusive).await;
        println!("exclusive open: {}", describe(&exclusive));
        assert!(matches!(exclusive, Err(HidError::Unsupported)));
    });
}

/// Records how IOKit answers competing opens. Run it with `--nocapture` and
/// keep the printed lines: the macOS statements in the documentation of
/// `Access` were measured with it (2026-10-11, one process).
#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs a YKUSH3 board"]
fn macos_exclusive_excludes_other_handles() {
    pollster::block_on(async {
        let device = ykush3().await;

        // Part 1: an exclusive handle is held first.
        {
            let mut held = open(&device, Access::Exclusive)
                .await
                .expect("exclusive open");
            let state = port_status(&mut held, "exclusive", 1).await;
            println!("exclusive open: Ok, port 1 state 0x{state:02x}");

            let second_exclusive = open(&device, Access::Exclusive).await;
            println!("exclusive open while held exclusively: {}", describe(&second_exclusive));

            let shared = open(&device, Access::Shared).await;
            println!("shared open while held exclusively: {}", describe(&shared));

            let default = timed(device.open()).await.expect("default open timed out");
            println!("default open while held exclusively: {}", describe(&default));

            let feature = timed(device.open_feature_handle())
                .await
                .expect("feature handle open timed out");
            println!("default feature handle while held exclusively: {}", describe(&feature));

            assert!(matches!(second_exclusive, Err(HidError::Busy)), "second exclusive open");
            assert!(matches!(shared, Err(HidError::Busy)), "shared open");
            assert!(matches!(default, Err(HidError::Busy)), "default open");
            assert!(matches!(feature, Err(HidError::Busy)), "default feature handle");
        }

        // Part 2a: two shared handles coexist. Each asks for a different port,
        // so an answer that reaches the wrong handle can not pass.
        {
            let mut first = open(&device, Access::Shared)
                .await
                .expect("first shared open");
            let mut second = open(&device, Access::Shared)
                .await
                .expect("second shared open while the first is held");
            let state = port_status(&mut first, "first shared", 1).await;
            println!("first of two shared handles: Ok, port 1 state 0x{state:02x}");
            let state = port_status(&mut second, "second shared", 2).await;
            println!("second of two shared handles: Ok, port 2 state 0x{state:02x}");
        }

        // Part 2b: a shared handle, and then a default one, is held first and
        // another handle seizes the device. The seize is not refused, the
        // holder's writes are refused with kIOReturnExclusiveAccess while the
        // seize is held, and the holder works again once it is released.
        let shared = open(&device, Access::Shared)
            .await
            .expect("shared open after the exclusive handle was dropped");
        seize_over(&device, shared, "shared").await;

        let default = timed(device.open())
            .await
            .expect("default open timed out")
            .expect("default open");
        seize_over(&device, default, "default").await;
    });
}

/// Seizes the device while `holder` has it open, checks that the holder is
/// refused with `Busy` during the seize and works again, without reopening,
/// after the seizing handle was dropped.
#[cfg(target_os = "macos")]
async fn seize_over(device: &Device, mut holder: DeviceReaderWriter, label: &str) {
    // Also shows that a previously dropped seizing handle released the seize.
    let state = port_status(&mut holder, label, 1).await;
    println!("{label} handle before the seize: Ok, port 1 state 0x{state:02x}");

    let exclusive = open(device, Access::Exclusive).await;
    println!("exclusive open while held {label}: {}", describe(&exclusive));
    let mut exclusive = exclusive.unwrap_or_else(|err| panic!("seize over a {label} handle: {err:?}"));

    let mut request = [0u8; REPORT_SIZE + 1];
    request[1] = PORT_STATUS | 1;
    let write = timed(holder.write_output_report(&request))
        .await
        .unwrap_or_else(|| panic!("write through the {label} handle during the seize timed out"));
    println!("write through the {label} handle during the seize: {}", describe(&write));
    assert!(matches!(write, Err(HidError::Busy)), "write through the {label} handle during the seize");

    let state = port_status(&mut exclusive, "seizing", 1).await;
    println!("status read through the seizing handle: Ok, port 1 state 0x{state:02x}");

    drop(exclusive);

    // The status read is a write followed by a read, both bounded. It asks
    // for port 2, which no request before it did, so only a fresh answer
    // passes.
    let state = port_status(&mut holder, label, 2).await;
    println!("{label} handle after the seize was released: Ok, port 2 state 0x{state:02x}");
}

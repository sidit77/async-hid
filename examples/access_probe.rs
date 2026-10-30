//! Probes how the access modes behave between processes, with a Yepkit YKUSH3
//! (vendor 0x04d8, product 0xf11b).
//!
//! ```text
//! cargo run --example access_probe -- hold <shared|exclusive|default> <secs>
//! cargo run --example access_probe -- probe <shared|exclusive|default>
//! ```
//!
//! `hold` opens the device, reads the status once and then every 500 ms until
//! `secs` have passed. `probe` opens the device and reads the status once.
//! Run a `hold` in one terminal and a `probe` or another `hold` in a second
//! one; every line names the role, the mode and the outcome. A refused open
//! is an outcome too and exits with 0; usage errors, a missing board and a
//! timed out open exit with 1.
//!
//! Read only: the board is only ever asked for the state of downstream ports
//! 1 and 2 (opcodes 0x21 and 0x22). No switching command is sent. The opcodes
//! and the answer layout are taken from the protocol table in the ykush3-rs
//! README, section "USB control protocol": 0x21 to 0x23 ask for the state of
//! port 1 to 3, byte 0 of the 64 byte answer is 0x01 on success, byte 1
//! carries the on/off state in the high nibble and the port number in the
//! low nibble. Every write and read is bounded by a timeout.
//!
//! The status reads alternate between port 1 and port 2, and an answer counts
//! only if its port number matches the request, so a stale answer to an
//! earlier request can not pass for a fresh one. Answers for the other port
//! are skipped, and their number is printed.

use std::future::Future;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use async_hid::{Access, AsyncHidRead, AsyncHidWrite, Device, DeviceReaderWriter, HidBackend};
use async_io::Timer;
use futures_lite::{future, StreamExt};

const YKUSH3_VID: u16 = 0x04d8;
const YKUSH3_PID: u16 = 0xf11b;
const REPORT_SIZE: usize = 64;
/// Opcode base of the port state requests: 0x20 | port.
const PORT_STATUS: u8 = 0x20;
const ACK: u8 = 0x01;
const TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

const USAGE: &str = "usage: access_probe hold <shared|exclusive|default> <secs>\n       access_probe probe <shared|exclusive|default>";

async fn timed<T>(op: impl Future<Output = T>) -> Option<T> {
    timed_for(TIMEOUT, op).await
}

async fn timed_for<T>(limit: Duration, op: impl Future<Output = T>) -> Option<T> {
    future::or(async { Some(op.await) }, async {
        Timer::after(limit).await;
        None
    })
    .await
}

/// Asks for the state of downstream `port` and describes the outcome.
async fn status(handle: &mut DeviceReaderWriter, port: u8) -> String {
    let mut request = [0u8; REPORT_SIZE + 1];
    request[1] = PORT_STATUS | port;
    match timed(handle.write_output_report(&request)).await {
        None => return format!("port {port}: write timed out"),
        Some(Err(err)) => return format!("port {port}: write failed: {:?}", err),
        Some(Ok(())) => {}
    }

    let deadline = Instant::now() + TIMEOUT;
    let mut skipped = 0;
    loop {
        let mut answer = [0u8; REPORT_SIZE];
        let remaining = deadline.saturating_duration_since(Instant::now());
        match timed_for(remaining, handle.read_input_report(&mut answer)).await {
            None => return format!("port {port}: read timed out ({skipped} skipped)"),
            Some(Err(err)) => return format!("port {port}: read failed: {:?}", err),
            Some(Ok(read)) if read != REPORT_SIZE || answer[0] != ACK => {
                return format!("port {port}: unexpected answer: {read} bytes, {:02x?}", &answer[..2]);
            }
            Some(Ok(_)) if answer[1] & 0x0f == port => {
                return format!("ok, port {port} state 0x{:02x} ({skipped} skipped)", answer[1]);
            }
            Some(Ok(_)) => skipped += 1
        }
    }
}

/// `None` stands for the platform default.
fn parse_mode(mode: &str) -> Option<Option<Access>> {
    match mode {
        "shared" => Some(Some(Access::Shared)),
        "exclusive" => Some(Some(Access::Exclusive)),
        "default" => Some(None),
        _ => None
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (role, mode, secs) = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["hold", mode, secs] => match secs.parse::<u64>() {
            Ok(secs) => ("hold", *mode, secs),
            Err(_) => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        },
        ["probe", mode] => ("probe", *mode, 0),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let Some(access) = parse_mode(mode) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    pollster::block_on(async {
        let device = match HidBackend::default().enumerate().await {
            Ok(mut devices) => {
                devices
                    .find(|d: &Device| d.vendor_id == YKUSH3_VID && d.product_id == YKUSH3_PID)
                    .await
            }
            Err(err) => {
                println!("{role} {mode}: enumeration failed: {:?}", err);
                return ExitCode::FAILURE;
            }
        };
        let Some(device) = device else {
            println!("{role} {mode}: no YKUSH3 attached");
            return ExitCode::FAILURE;
        };

        let opened = match access {
            Some(access) => timed(device.open_options().access(access).open()).await,
            None => timed(device.open()).await
        };
        let mut handle = match opened {
            None => {
                println!("{role} {mode}: open timed out");
                return ExitCode::FAILURE;
            }
            Some(Err(err)) => {
                println!("{role} {mode}: open failed: {:?}", err);
                return ExitCode::SUCCESS;
            }
            Some(Ok(handle)) => handle
        };
        println!("{role} {mode}: open ok, status {}", status(&mut handle, 1).await);

        let end = Instant::now() + Duration::from_secs(secs);
        let mut port = 1;
        while Instant::now() < end {
            Timer::after(POLL_INTERVAL).await;
            port = if port == 1 { 2 } else { 1 };
            println!("{role} {mode}: status {}", status(&mut handle, port).await);
        }
        ExitCode::SUCCESS
    })
}

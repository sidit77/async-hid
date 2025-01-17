#!/usr/bin/env just --justfile

set windows-shell := ["powershell.exe", "-c"]

fmt:
  cargo +nightly fmt

check-windows-winrt:
    cargo check --no-default-features --features=winrt --target x86_64-pc-windows-msvc

check-windows-win32:
    cargo check --no-default-features --features=win32 --target x86_64-pc-windows-msvc

check-windows: check-windows-winrt check-windows-win32

check-linux-asyncio:
    cargo check --no-default-features --features=async-io --target x86_64-unknown-linux-gnu

check-linux-tokio:
    cargo check --no-default-features --features=tokio --target x86_64-unknown-linux-gnu

check-linux: check-linux-asyncio check-linux-tokio

check-macos:
    cargo check --no-default-features --target x86_64-apple-darwin

check: check-windows check-linux check-macos

# lint-windows:
#     cargo clippy --all-features --target x86_64-pc-windows-msvc
#
# lint-linux:
#     cargo clippy --all-features --target x86_64-unknown-linux-gnu
#
# lint-macos:
#     cargo clippy --all-features --target x86_64-apple-darwin
#
# lint: lint-windows lint-linux lint-macos

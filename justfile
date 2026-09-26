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

check-freebsd-asyncio:
    cargo check --no-default-features --features=async-io --target x86_64-unknown-freebsd

check-freebsd-tokio:
    cargo check --no-default-features --features=tokio --target x86_64-unknown-freebsd

check-freebsd: check-freebsd-asyncio check-freebsd-tokio

check-macos:
    cargo check --no-default-features --target x86_64-apple-darwin

check: check-windows check-linux check-freebsd check-macos

lint-windows:
    cargo clippy --all-features --target x86_64-pc-windows-msvc

lint-linux:
    cargo clippy --no-default-features --features tokio --target x86_64-unknown-linux-gnu
    cargo clippy --no-default-features --features async-io --target x86_64-unknown-linux-gnu

lint-freebsd:
    cargo clippy --no-default-features --features tokio --target x86_64-unknown-freebsd
    cargo clippy --no-default-features --features async-io --target x86_64-unknown-freebsd

lint-macos:
    cargo clippy --all-features --target x86_64-apple-darwin

lint: lint-windows lint-linux lint-freebsd lint-macos

# The tests this host can run. Every backend is type checked by `check` and
# `lint` for all four targets; only the host's own backend can actually be
# built, linked and executed, so that is what these do. The freebsdhid parser
# tests are unreachable everywhere: they are gated on the target and there is
# no FreeBSD runner.

[linux]
test-host:
    cargo test --lib --no-default-features --features async-io
    cargo test --lib --no-default-features --features tokio
    cargo test --doc --no-default-features --features async-io

[macos]
test-host:
    cargo test --lib
    cargo test --doc

[windows]
test-host:
    cargo test --lib --no-default-features --features win32
    cargo test --lib --no-default-features --features winrt
    cargo test --doc --no-default-features --features win32

# Everything CI runs, as far as one host can. CI additionally runs `test-host`
# on Linux, macOS and Windows; here it runs on this host only. Needs the cross
# targets:
#
#   rustup target add x86_64-apple-darwin x86_64-pc-windows-msvc \
#                     x86_64-unknown-linux-gnu x86_64-unknown-freebsd
ci: check lint test-host

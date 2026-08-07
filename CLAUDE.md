# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rust library and CLI (`arb`) for controlling the ABACOM CH341A USB relay board. Dual-mode crate: library by default, binary with the `build-binary` feature flag.

## Build Commands

```bash
# Library only
cargo build

# With CLI binary
cargo build --features=build-binary

# Release build with binary
cargo build --features=build-binary --release

# Linting
cargo clippy
cargo clippy --features=build-binary   # include binary code

# Formatting
cargo fmt

# Tests
cargo test --features=build-binary
```

Tests run against a simulated A6275 (`FakeA6275` in `src/lib.rs`), so no hardware
is needed. CI runs fmt, the tests, both clippy feature sets, and `cargo doc` with
`RUSTDOCFLAGS=-D warnings`.

## Architecture

Four-layer design:

1. **Public API** (`src/lib.rs`) — Three functions: `active_relays()`, `set_relays()`, `reset()`. Each takes an optional USB port number to disambiguate multiple boards.
2. **Relay addressing** (`src/relays.rs`) — `Relay` (one relay, 1–8) and `Relays` (a set of them). `Relay::bit()` is the only place the relay-number-to-bit mapping is written down.
3. **CH341A protocol** (`src/ch341a.rs`) — Low-level USB bulk transfers via `rusb`. No `unsafe` code. Exposes `set_output()` and `get_input()` through the `Gpio` trait, which lets the shift register protocol be tested without hardware.
4. **CLI** (`src/bin/arb.rs`) — `clap`-derived argument parser. Only compiled with `build-binary` feature.

Error types live in `src/errors.rs` using `thiserror`.

## Key Details

- The board stores relay state as an 8-bit shift register mask (bit 0 = relay 1, bit 7 = relay 8; `1` = active). Outside `Relay::bit()`, work with `Relay`/`Relays` rather than raw bits
- Communication uses the Allegro A6275 shift register protocol (SPI-like: DATA, CLK, LATCH pins on CH341A GPIO lines D5, D3, D0; serial out read from D7)
- CH341A uses two commands: `0xA1` (set D0–D7 outputs) and `0xA0` (read D0–D7 inputs, returns 6 bytes but only byte 0 is used)
- USB device identified by vendor `0x1a86` / product `0x5512`
- `Verify::Enabled` makes `set_relays()` read back the shift register after latching and compare
- `active_relays()` includes a health check: writes an inverted test pattern to the shift register (without latching) and verifies the read-back
- `0` is a CLI-only spelling of "all off" — the library has no such relay. Keep that sentinel in `src/bin/arb.rs`
- Requires system `libusb` at compile time

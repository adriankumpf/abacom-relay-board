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

Six-layer design — `Usb` finds, `Board` is the device, `A6275` is the protocol, `Ch341a` is the wire:

1. **Public API** (`src/lib.rs`) — `Usb` is a libusb context and finds boards; `usb.board(port)` returns a `Board`, which has `relays()`, `set_relays()`, `self_test()`, `reset_device()` and `port()`. The optional USB port number disambiguates multiple boards, and `usb.boards()` enumerates them all.
2. **Relay addressing** (`src/relays.rs`) — `Relay` (one relay, 1–8) and `Relays` (a set of them). `Relay::bit()` is the only place the relay-number-to-bit mapping is written down.
3. **Board selection** (`src/find.rs`) — `Path` (where a board sits on the USB tree) and `Select` (which board a `Board` names). `find_devices()` enumerates, `find_device()` resolves one. Crate-private.
4. **Shift register protocol** (`src/a6275.rs`) — `A6275<T: Gpio>` clocks bits in and out, and owns the A6275 pin map (`LATCH`, `CLK`, `DATA`, `READ`). Crate-private, like `Path`/`Select`: both are exercised by the tests directly, since neither needs hardware.
5. **CH341A protocol** (`src/ch341a.rs`) — Low-level USB bulk transfers via `rusb`. No `unsafe` code. Exposes `set_output()` and `sample_clocked()` through the `Gpio` trait, which lets the shift register protocol be tested without hardware.
6. **CLI** (`src/bin/arb.rs`) — `clap`-derived argument parser. Only compiled with `build-binary` feature. Modes (`--status`, `--list`, `--reset`, relays) are mutually exclusive via one `ArgGroup`.

Error types live in `src/errors.rs` using `thiserror`.

## Key Details

- The board stores relay state as an 8-bit shift register mask (bit 0 = relay 1, bit 7 = relay 8; `1` = active). Outside `Relay::bit()`, work with `Relay`/`Relays` rather than raw bits
- Communication uses the Allegro A6275 shift register protocol (SPI-like: DATA, CLK, LATCH pins on CH341A GPIO lines D5, D3, D0; serial out read from D7)
- CH341A uses two commands: `0xA1` (set D0–D7 outputs, one transfer per line change) and `0xAB` (UIO stream — a short program of pin states in one packet, used to read the whole shift register in two transfers). Reading the register is batched; writing it is not, because the CH341A outruns the DATA line's slew rate
- USB device identified by vendor `0x1a86` / product `0x5512`
- `Verify::Enabled` makes `Board::set_relays()` read back the shift register after latching and compare
- `Board::relays()` is a plain read; `Board::self_test()` is the separate health check, which writes an inverted test pattern to the shift register (without latching), verifies the read-back and restores the register. It costs as much again as a read, which is why it is not on the read path
- `Usb::new()` is expensive (~6.5 ms, almost entirely `libusb_init`) and everything else per call is ~50 µs, so a context is meant to be created once and kept. `Board` holds a private `Select` — any board, a port number, or a full bus-and-hub `Path` — never a resolved device or a claim: it finds and claims per call, so it never locks another application out of a shared board
- A USB port number is the board's port on its *parent hub*, so it is not unique across hubs. `usb.board(Some(p))` matches on it anyway (`MultipleFound` is the honest answer to a collision); `usb.boards()` selects by `Path` instead, so enumerated boards never collide
- `0` is a CLI-only spelling of "all off" — the library has no such relay. Keep that sentinel in `src/bin/arb.rs`
- Requires system `libusb` at compile time

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

Tests run against a simulated A6275 (`FakeA6275` in `src/a6275.rs`), so no hardware
is needed. CI runs fmt, the tests, both clippy feature sets, and `cargo doc` with
`RUSTDOCFLAGS=-D warnings`.

## Architecture

Six-layer design — `Usb` finds, `Board` is the device, `A6275` is the protocol, `Ch341a` is the wire:

1. **Public API** (`src/lib.rs`) — `Usb` is a libusb context and finds boards; `usb.board(port)` returns a `Board`, which has `relays()`, `set_relays()`, `self_test()`, `reset_device()` and `port()`. The optional USB port number disambiguates multiple boards, and `usb.boards()` enumerates them all.
2. **Relay addressing** (`src/relays.rs`) — `Relay` (one relay, 1–8) and `Relays` (a set of them). `Relay::bit()` is the only place the relay-number-to-bit mapping is written down.
3. **Board selection** (`src/find.rs`) — `Location` (where a board sits on the USB tree; public, and the only public type that does not live in `lib.rs`) and `Select` (which board a `Board` names; crate-private). `find_devices()` enumerates, `find_device()` resolves one.
4. **Shift register protocol** (`src/a6275.rs`) — `A6275<T: Gpio>` clocks bits in and out, and owns the A6275 pin map (`LATCH`, `CLK`, `DATA`, `READ`). Crate-private, like `Select`: both are exercised by the tests directly, since neither needs hardware.
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
- `Usb::new()` is expensive (~6.5 ms, almost entirely `libusb_init`) and everything else per call is ~50 µs, so a context is meant to be created once and kept. `Board` holds a private `Select` — any board, a port number, or a full bus-and-hub `Location` — never a resolved device or a claim: it finds and claims per call, so it never locks another application out of a shared board
- A USB port number is the board's port on its *parent hub*, so it is not unique across hubs. `usb.board(Some(p))` matches on it anyway (`MultipleFound` is the honest answer to a collision); `usb.boards()` selects by `Location` instead, so enumerated boards never collide
- `Location` is the only *storable* board identifier: `Display`/`FromStr` round-trip through the `lsusb -t` spelling (`1-1.3`), which is what lets a consumer keep "which board" in a config file. `Board::location()` is `Some` only for an enumerated board — `usb.board(Some(3))` has nothing stable to report, and returning its port there would hand back exactly the ambiguity `Location` exists to remove. Keep `Display for Location` and `FromStr for Location` in step; the round-trip test in `src/find.rs` is what pins them
- `0` is a CLI-only spelling of "all off" — the library has no such relay. Keep that sentinel in `src/bin/arb.rs`
- Requires system `libusb` at compile time
- Measured on two boards (2026-08-08), within 4% of each other: **41–43 µs per USB transfer**, and a `Usb::new()` + open + claim + release dance of 6.545 ms of which `libusb_init` is 99.2%. Transfer counts in the docs are derived from these

## Settled — do not re-open

Investigated and rejected. The reasons are not visible from the code, which is why they are written down.

- **Batching the write path into a UIO stream.** D5 slew: the CH341A emits stream states faster than DATA settles, so the rising clock edge samples the previous bit. Reproduced byte-for-byte on two boards; splitting and padding both improve it and neither converges. Only the read path is batched
- **Three writes per bit down to two.** The third write only brings CLK low, which the next iteration's first write already does. The *logic* half is settled — the change passes the full suite — so what is left is purely electrical: it moves the falling clock edge onto the data transition, on the same D5 that breaks the batched write. Worth 0.33 ms, and wants a bench check nobody has spent
- **A separate `UIO_STM_DIR` transfer at open.** Measured at 38.99 µs against 41.44 µs for an ordinary transfer, it nearly doubles the ~50 µs of per-call overhead — and is paid by every call, including `reset_device()`, which issues no GPIO at all. Both GPIO paths carry the line directions themselves (see `OUTPUT_LINES` in `src/ch341a.rs`), so `Ch341a::open` sends nothing
- **A board handle that holds the USB claim.** `claim_interface` is exclusive and boards are shared between applications, so a handle keeping it ends sharing rather than degrading it. `Board` is deliberately not this: it stores a selector and claims per call
- **Caching the resolved device inside `Board`.** 4 µs, against a handle that goes stale on hot-plug
- **Naming an enumerated board by `Device::address()`.** It is reassigned on re-enumeration, so a listed `Board` would go stale after `reset_device()` — which the downstream consumer calls on every retry. A `Location` survives that
- **Sorting a `Vec` in `find_devices` instead of keying a `BTreeMap`.** The reason is on `find_devices` itself; what is not written there is that `find_devices` needs hardware to test, so a dropped sort would not fail anything
- **Partial updates (`board.turn_on(relay)`).** The hardware latches all 8 bits at once, so this would hide a read-modify-write behind a setter that looks atomic — actively dangerous on a shared board
- **`CH341A_CMD_SPI_STREAM` (0xA8).** Byte-oriented SPI with hardware bit ordering — the wrong shape for bit-banging a shift register's latch
- **A library-internal or `rusb::GlobalContext` singleton.** `GlobalContext` panics if `libusb_init` fails (fatal inside a NIF) and can never be rebuilt, so the drop-and-recreate recovery path would not exist. Any singleton also makes a policy decision on every consumer's behalf that the one-call-per-process CLI cannot use
- **Configurable USB timeouts.** Considered for 0.8.0 and dropped; `TIMEOUT_READ`/`TIMEOUT_WRITE` stay constants at 1000 ms

## Known Limitation

The library is atomic *within* a call but not *between* calls: `relays()` followed by `set_relays()` is two claims, so on a shared board a write can land in the gap and one side's change is silently lost. Documented for consumers under `# Atomicity` on `Board` (`src/lib.rs`) — keep the two in step. Closing it rather than documenting it would need a scoped claim (`board.with_claim(|claimed| …)`) holding one `Ch341a` across a closure, which `Board` has room for without a redesign.

## Testing

- **Mutation-check new protocol logic** — a passing suite that catches nothing is worse than none. Break it deliberately and confirm the failure before claiming coverage: an LSB-first `shift_out_bits`, an LSB-first register read, a dropped register restore and a sample taken after the rising edge are all caught today
- **One known mutation survivor:** `relays()` quietly self-testing again. `Board`'s methods go through `claim()` and need hardware, so the tests drive `A6275` one layer down; the transfer-count assertions (`status()` 28, `self_test()` 56) are what pin the cost there
- **Doctests are the anti-drift mechanism** for the relay mapping — a `no_run` example once shipped a wrong bit literal precisely because nothing executed it
- **Hardware safety.** Anything driving a real board must never assert LATCH (`src/a6275.rs`), or writes reach the relay outputs. Reading *is* destructive — zeros shift in — so capture the register at startup and restore it on every exit path including panic unwind, otherwise the next read lies without any relay having moved

## Consumers

[arb-ex](https://github.com/adriankumpf/arb-ex) is the only Rust-level consumer and pins a git tag, so every tag bump costs a coordinated Elixir release — batch breaking changes into one release rather than paying repeatedly. It must hold one `Usb` for the whole BEAM node (that is where the ~6.5 ms/call saving is) in something swappable, since a reused context no longer self-heals: after repeated failures the holder drops it and builds a new one.

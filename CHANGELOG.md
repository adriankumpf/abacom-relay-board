# Changelog

## [Unreleased]

### Changed (**breaking**)

- Replace the `active_relays`, `set_relays` and `reset` free functions with two
  types: `Usb`, a libusb context, and `Board`, one relay board found through it.
  Each free function created its own context, and `libusb_init` was 99% of the
  cost of a call — 6.5 ms of a 6.6 ms open. A caller that keeps one `Usb` now
  pays that once instead of per call, leaving ~50 µs of per-call overhead:

  ```rust
  let usb = arb::Usb::new()?;
  let board = usb.board(port);

  board.set_relays(relays, Verify::Enabled)?;
  let active = board.relays()?;
  board.reset_device()?;
  ```

  `Usb` is cheap to clone and `Send + Sync`, so one context can serve every
  thread. `Board` is free to build, resolves nothing until a method is called,
  and — as before — claims the USB interface only for the duration of one call,
  so it never locks another application out of a shared board. Note that a
  reused context no longer self-heals: after repeated failures, drop the `Usb`
  and build a new one
- Rename `get_status` to `Board::relays`, now returning `Relays`
- Rename `set_status` to `Board::set_relays`, now taking `Relays`
- Rename `reset` to `Board::reset_device`. `reset(port)` read like "turn all the
  relays off"; it is a USB reset and leaves the relay outputs untouched
- Model relays as types instead of a raw bitmask. `Relay` names a single relay
  (1–8) and `Relays` a set of them, so the relay-number-to-bit mapping is stated
  once in the library rather than re-derived by each consumer
- Replace `set_relays`' `verify: bool` parameter with a `Verify` enum
- Remove `Error::IO`, along with the `From<std::io::Error>` conversion it
  provided. No library path could produce it: it existed only so the CLI could
  use `arb::Result` for its own stdout writes, which every consumer then had to
  handle as an unreachable variant. The CLI now uses `Box<dyn Error>`
- Mark `Error` as `#[non_exhaustive]`, so that future variants can be added
  without another breaking release. Downstream `match`es need a wildcard arm
- Remove the default type parameter from `Result`, which is now written
  `Result<()>` rather than a bare `Result`. The default made rustdoc render the
  return type of `set_relays` and `reset_device` as `Result`, with no way to
  tell what it resolved to
- Rename the `Iter` re-export to `RelayIter`. `arb::Iter` was too vague a name
  for the crate root
- Give `Error::VerificationFailed` `expected` and `actual` fields, both `Relays`.
  It previously carried nothing, so a caller was told the read-back disagreed but
  not how
- Rename `Error::BadDevice` to `Error::SelfTestFailed`, and correct its
  documentation, which still described an "empty read" — that became
  `Error::UnexpectedTransferLength` in 0.7.1. The two read-back failures stay
  distinct because the recovery differs: `SelfTestFailed` writes its test pattern
  without latching, so the relays were never touched, whereas
  `VerificationFailed` latched first and leaves their physical state unknown
- Report a board held by another application as the new `Error::Busy` rather than
  as `Error::Usb(rusb::Error::Busy)`, which made it indistinguishable from a real
  USB fault even though it is a normal and retryable condition on a shared board.
  Breaking for callers: a `match` arm on `Error::Usb(rusb::Error::Busy)` stops
  firing and falls through to the wildcard `#[non_exhaustive]` already requires
- Render `Relays::NONE` as `none` rather than as the empty string. `Display` is
  what error messages and `arb --status` interpolate, and an empty set previously
  rendered as nothing at all — `Active relays: ` — which reads as a bug rather
  than as "no relays"

### Changed

- Read the shift register in a single CH341A UIO stream instead of one USB
  transfer per line change. A read costs 2 transfers rather than 33 — one packet
  of pin states out, one packet of samples back — which takes `Board::relays()`
  from 118 transfers to 56 and `set_relays` with `Verify::Enabled` from 87 to
  56: roughly 4.9 ms to 2.4 ms and 3.6 ms to 2.4 ms at the measured ~41 µs per
  transfer. The stream claims the lines it drives, so opening a board still sends
  nothing. The write path is deliberately left alone: the CH341A emits stream
  states faster than the DATA line settles, so a batched write clocks in the
  previous bit

### Added

- `Error::InvalidRelay` for relay numbers outside 1–8

### Fixed

- Correct the relay bitmask in the `set_status` documentation example
- Raise the USB bulk read timeout from 10 ms to 1000 ms, matching the write
  timeout. Ten milliseconds for a USB round trip is tight enough to fail
  spuriously on a loaded host or through a hub, and nothing retries behind it
- Re-attach the kernel driver when the interface is released. The driver was
  previously detached on open and never restored

## [0.7.1] - 2026-04-02

### Fixed

- Validate USB bulk transfer lengths instead of accepting short or partial transfers
- Return a dedicated error for unexpected USB transfer lengths

### Changed

- Bump `clap` to 4.6.0

## [0.7.0] - 2026-02-21

### Bug Fixes

- Fix inverted `disable_verification` flag
- Fix relay range to allow relay 8
- Fix swapped USB endpoint names
- Fix unchecked indexing on USB read result in `get_status`
- Fix CLI argument validation: add conflicts and remove silent default

### Improvements

- Remove all unsafe code from `ch341a.rs`
- Simplify `ch341a.rs`: inline helpers and use stack arrays
- Return `u8` from `get_input` and add documentation (**breaking**)
- Bump deps and fix warnings

## [0.6.0] - 2025-04-10

- Bump Rust edition to 2024
- Fix new clippy warnings
- Bump dependencies

## [0.5.3] - 2024-03-02

- Update dependencies

## [0.5.2] - 2022-04-16

### Changed

- Bump rusb to 0.9
- Migrate to clap v3

## [0.5.1] - 2021-03-19

### Changed

- Bump rusb to 0.8

## [0.5.0] - 2019-12-20

### Changed

- Switch from [libusb-rs](https://github.com/dcuddeback/libusb-rs) to [rusb](https://github.com/a1ien/rusb)

## [0.4.0] - 2019-05-23

### Changed

- Switch to fork of `libusb-rs`

### Removed

- Remove explicit lifetimes

## [0.3.0] - 2019-01-06

### Changed

- Use `writeln!` macro to write to stdout
- Use `structopt` crate to parse command line arguments
- Transition to rust 2018

### Removed

- Drop support for Rust 1.30 and below

## [0.2.2] - 2018-08-20

### Changed

- Update dependencies

## [0.2.1] - 2018-05-16

### Changed

- Use new rust syntax features and update dependencies

## [0.2.0] - 2018-04-08

### Added

- Introduce `arb::reset` to reset a relay board

### Changed

- Hide binary compilation behind feature flag `build-binary`

## [0.1.0] - 2018-04-01

[unreleased]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.7.1...HEAD
[0.7.1]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.5.3...v0.6.0
[0.5.3]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/adriankumpf/abacom-relay-board/compare/4c44ae3...v0.1.0

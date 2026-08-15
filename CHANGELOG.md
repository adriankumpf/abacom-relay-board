# Changelog

## [Unreleased]

### Migrating from 0.7.1

Every 0.7.1 entry point is gone: the three free functions are methods on two new
types, and relays are named rather than masked. Build one `Usb` and keep it —
that is where the release's headline saving lives.

```rust
let usb = arb::Usb::new()?;  // expensive; do this once
let board = usb.board(port);

board.set_relays(Relay::One | Relay::Three, Verify::Enabled)?;
let active = board.relays()?;
board.reset_device()?;
```

| 0.7.1                                  | 0.8.0                                                             |
| -------------------------------------- | ----------------------------------------------------------------- |
| `arb::get_status(port)?` → `u8`        | `usb.board(port).relays()?` → `Relays` — **no longer self-tests** |
| `arb::set_status(bits, verify, port)?` | `usb.board(port).set_relays(relays, Verify::Enabled)?`            |
| `arb::reset(port)?`                    | `usb.board(port).reset_device()?`                                 |
| `arb::Result`                          | `arb::Result<()>`                                                 |
| `Error::BadDevice`                     | `Error::SelfTestFailed`                                           |
| `Error::Usb(rusb::Error::Busy)`        | `Error::Busy`                                                     |
| `Error::IO`                            | gone — the CLI owns its I/O errors                                |
| —                                      | `match` on `Error` needs a wildcard arm (`#[non_exhaustive]`)     |

Code holding raw masks converts at the boundary with `Relays::from_bits(u8)` and
`Relays::bits()`. Code building sets is better served by `Relay::try_from(n)?`,
`Relay::One | Relay::Three`, or `collect()` from an iterator of `Relay`;
`Relays::iter()` is the way back out.

### Changed (**breaking**)

- Replace the three free functions with two types: `Usb`, a libusb context, and
  `Board`, one relay board found through it. Each free function created its own
  context, and `libusb_init` was 99% of the cost of a call — 6.5 ms of a 6.6 ms
  open. A caller that keeps one `Usb` now pays that once instead of per call,
  leaving ~50 µs of per-call overhead. `Usb` is cheap to clone and `Send + Sync`,
  so one context can serve every thread. `Board` is free to build, resolves
  nothing until a method is called, and — as before — claims the USB interface
  only for the duration of one call, so it never locks another application out of
  a shared board. Note that a reused context no longer self-heals: no error
  reliably says a context has gone sour, so rather than trying to classify one,
  drop the `Usb` and build a new one after *any* failed operation — it costs
  ~6.5 ms and failures are rare
- Rename the free functions to `Board::relays`, `Board::set_relays` and
  `Board::reset_device`. `reset(port)` in particular read like "turn all the
  relays off"; it is a USB reset and leaves the relay outputs untouched
- Model relays as types instead of a raw bitmask. `Relay` names a single relay
  (1–8) and `Relays` a set of them, so the relay-number-to-bit mapping is stated
  once in the library rather than re-derived by each consumer. `Relays::from_bits`
  and `Relays::bits` are the only places the mask is still exposed
- Replace `set_relays`' `verify: bool` parameter with a `Verify` enum.
  `Verify::Enabled` is its `Default`, so verification stays the safe option a
  caller gets without asking
- Mark `Error` as `#[non_exhaustive]`, so that future variants can be added
  without another breaking release. Downstream `match`es need a wildcard arm
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
  firing
- Split the board's self-test out of `Board::relays`, which is now a plain read.
  Reading performed a hidden read-modify-write — an inverted test pattern written
  to the shift register, read back and undone — that doubled its cost and was not
  mentioned by its name. The check is now `Board::self_test()`, and a read costs
  28 transfers rather than 56 — roughly 2.3 ms to 1.1 ms. **Callers that relied
  on reading to vet the board must call `self_test()` themselves**;
  `arb --status` still does
- Render `Relays::NONE` as `none` rather than as the empty string. `Display` is
  what error messages and `arb --status` interpolate, and an empty set previously
  rendered as nothing at all — `Active relays: ` — which reads as a bug rather
  than as "no relays"

### Removed (**breaking**)

- `Error::IO`, along with the `From<std::io::Error>` conversion it provided. No
  library path could produce it: it existed only so the CLI could use
  `arb::Result` for its own stdout writes, which every consumer then had to
  handle as an unreachable variant
- The default type parameter on `Result`, which is now written `Result<()>`
  rather than a bare `Result`. The default made rustdoc render the return type of
  `set_relays` and `reset_device` as `Result`, with no way to tell what it
  resolved to

### Changed

- Read the shift register in a single CH341A UIO stream instead of one USB
  transfer per line change — one packet of pin states out, one packet of samples
  back, 2 transfers rather than 33. That takes `set_relays` with
  `Verify::Enabled` from 87 transfers to 56 and `Board::self_test()` from 118 to
  56: roughly 3.6 ms to 2.3 ms and 4.8 ms to 2.3 ms at the measured ~41 µs per
  transfer. The write path is deliberately left alone, because the CH341A drives
  a batched write faster than the DATA line settles
- Print CLI errors as `arb: <message>` on stderr — `arb: no relay board found`
  where it previously said `Error: NotFound`. Exit codes are unchanged

### Added

- `Relay`, `Relays` and `RelayIter`, the types that replace the raw mask.
  `Relays` is a `Copy` set with `NONE`/`ALL`, `insert`/`remove`/`contains`, `|`
  and `|=` over both `Relay` and `Relays`, `FromIterator`/`IntoIterator`, and a
  `Display` that renders `1 3 8`. `Relay::try_from(u8)` validates a relay number
  and `Relay::ALL` names all eight
- `Usb::boards()`, which returns every attached board in a stable order — there
  was previously no way to enumerate, and a host with four boards is a real
  configuration. An enumerated board is identified by where it sits on the USB
  tree rather than by port number, which is only unique among one hub's ports, so
  it always resolves back to the board it came from. `usb.board(Some(3))` still
  matches on port alone, exactly as before, with `MultipleFound` the answer to a
  collision and `boards()` the way out. An empty list means no board is attached
  rather than `Error::NotFound`
- `Board::port()`, so a caller can label the board it got, and `Display for
  Board`, which renders an enumerated board as `port 3 (1-1.3)` — enough to tell
  apart two boards that share a port number. The notation is the one `lsusb -t`
  uses, so `arb --list` can be read beside it: these boards carry no serial
  number and no product strings, so where a board is plugged in is the only
  thing that distinguishes two of them
- `arb --list`, which prints one line per attached board. Prints nothing when
  there is none, so the output stays readable line by line
- `Board::self_test()`, the read-back check that `Board::relays` used to perform
  on the way past. It moves no relay, so it is safe to call on a live board.
  A diagnostic rather than a guard on the operating path: `set_relays` with
  `Verify::Enabled` already covers the same ground inside one claim, and on the
  value the caller actually asked for, so reach for `self_test()` at startup,
  from a health check, or when a board is suspect
- `Error::InvalidRelay` for relay numbers outside 1–8

### Fixed

- Stop discarding the shift register restore when reading. Reading the A6275
  clocks zeros in, so the read primitive repaired the register afterwards — and
  the caller then overwrote that repair immediately, wasting ~31% of the USB
  traffic a read cost. No `LATCH` is asserted in any of the removed writes, so no
  relay could ever have moved
- Restore the shift register when a self-test fails. The check returned on
  mismatch before putting the register back, leaving it holding the zeros the
  read shifted in — so a failure made the *next* read disagree with the latched
  outputs, on exactly the board already suspected of misreporting. No relay moved
  either way
- Discard a stale UIO stream response after a clocked read fails. The device
  queues the stream's answer as the stream runs, so a read that timed out left
  eight bytes in the IN endpoint that nothing consumed, and the next read took
  them for its own: from then on every read reported the register as of one call
  earlier, spuriously failing verification, and the lengths matched so the
  transfer-length check could not see it. The endpoint is now drained on that
  error path
- Correct three pieces of documentation that would send a consumer's recovery the
  wrong way: `Error::MultipleFound` said it meant no port was given, when a port
  that two hubs both have a board on produces it as well (and supplying the port
  again cannot help, `Usb::boards` can); `Display for Board` documented a format
  it does not render; and `Gpio::set_output` claimed to drive D0–D7 when the
  direction mask makes D6 and D7 inputs
- Correct a wrong relay bitmask in a documentation example
- Raise the USB bulk timeouts to 1000 ms, from 10 ms for reads and 100 ms for
  writes. Ten milliseconds for a USB round trip is tight enough to fail
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

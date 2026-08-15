# Changelog

## [Unreleased]

A new public API. The three free functions become methods on two types, relays
are named instead of masked, and a caller that keeps one `Usb` around pays
libusb's start-up cost once rather than on every call.

### Migrating from 0.7.1

```rust
use arb::{Relay, Relays, Usb, Verify};

let usb = Usb::new()?;        // expensive: build one and keep it
let board = usb.board(port);  // free: finds and claims the board per call

board.set_relays(Relay::One | Relay::Three, Verify::Enabled)?;
let active = board.relays()?;
board.reset_device()?;
```

| 0.7.1                                  | 0.8.0                                                             |
| -------------------------------------- | ----------------------------------------------------------------- |
| `arb::get_status(port)?` → `u8`        | `usb.board(port).relays()?` → `Relays` (no longer self-tests)     |
| `arb::set_status(bits, verify, port)?` | `usb.board(port).set_relays(relays, Verify::Enabled)?`            |
| `arb::reset(port)?`                    | `usb.board(port).reset_device()?`                                 |
| `arb::Result`                          | `arb::Result<()>`                                                 |
| `Error::BadDevice`                     | `Error::SelfTestFailed`                                           |
| `Error::Usb(rusb::Error::Busy)`        | `Error::Busy`                                                     |
| `Error::IO`                            | gone: the CLI owns its I/O errors                                 |
| `match` on `Error`                     | needs a wildcard arm (`#[non_exhaustive]`)                        |

Code holding raw masks converts at the boundary with `Relays::from_bits(u8)` and
`Relays::bits()`. Code building sets is better served by `Relay::try_from(n)?`,
`Relay::One | Relay::Three`, or `collect()` from an iterator of `Relay`;
`Relays::iter()` is the way back out.

### Added

- `Relay` and `Relays`, the types that replace the raw mask. `Relays` is a `Copy`
  set with `NONE`/`ALL`, `insert`/`remove`/`contains`, `|` and `|=` over both
  `Relay` and `Relays`, `FromIterator`/`IntoIterator`, and a `Display` that
  renders `1 3 8`. `Relay::try_from(u8)` validates a relay number and
  `Relay::ALL` names all eight
- `Usb::boards()`, which returns every attached board in a stable order. There
  was previously no way to enumerate, and a host with four boards is a real
  configuration. An enumerated board is identified by where it sits on the USB
  tree rather than by port number, which is only unique among one hub's ports, so
  it always resolves back to the board it came from. An empty list means no board
  is attached rather than `Error::NotFound`
- `Board::port()` and `Display for Board`, which renders an enumerated board as
  `port 3 (1-1.3)`. These boards carry no serial number and no product strings,
  so where a board is plugged in is the only thing that tells two of them apart.
  The notation is the one `lsusb -t` uses, so `arb --list` can be read beside it
- `arb --list`, one line per attached board. Prints nothing when there is none,
  so the output stays readable line by line
- `Board::self_test()`, the read-back check that reading used to perform on the
  way past. It latches nothing, so it is safe to call on a live board, and it
  returns the `Relays` it found while checking: a caller wanting both a verdict
  and a state gets them from one claim, which is what `arb --status` now does.
  Reach for it at startup, from a health check, or when a board is suspect, not
  on the path of ordinary reads: `set_relays` with `Verify::Enabled` already
  checks the value the caller actually asked for
- `Error::InvalidRelay`, for relay numbers outside 1–8
- `Error::RegisterOutOfSync`, for a read that could not put the shift register
  back (see *Fixed*). It moves no relay of its own, but a later `relays()` can
  succeed and report relays as inactive while they are energized, so retrying the
  read is the one remedy that does not work. Write a known state with
  `set_relays` instead
- Package metadata for publishing, and a declared minimum Rust version of 1.85

### Changed (**breaking**)

- Replace the three free functions with two types: `Usb`, a libusb context, and
  `Board`, one relay board found through it. Each free function created its own
  context, and `libusb_init` was 6.5 ms of the 6.6 ms that cost. A caller that
  keeps one `Usb` pays it once instead of per call, leaving ~50 µs of per-call
  overhead. `Usb` is cheap to clone and `Send + Sync`, so one context serves every
  thread. `Board` is free to build and, as before, claims the USB interface only
  for the duration of one call, so it never locks another application out of a
  shared board. One caveat: a reused context does not self-heal, and no error
  reliably says a context has gone sour, so drop the `Usb` and build a new one
  after *any* failed operation
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
- Make `Board::relays()` a plain read. It used to perform a hidden
  read-modify-write (an inverted test pattern written to the shift register, read
  back and undone) that doubled its cost without being mentioned by its name. A
  read now costs 28 USB transfers rather than 56, roughly 1.1 ms instead of
  2.3 ms. **Callers that relied on reading to vet the board must call
  `self_test()` themselves**; `arb --status` does
- Report a board held by another application as `Error::Busy` rather than as
  `Error::Usb(rusb::Error::Busy)`, which made it indistinguishable from a real USB
  fault even though it is a normal and retryable condition on a shared board. A
  `match` arm on the old shape stops firing
- Rename `Error::BadDevice` to `Error::SelfTestFailed`. The two read-back failures
  stay distinct because the recovery differs: `SelfTestFailed` writes its test
  pattern without latching, so the relays were never touched, whereas
  `VerificationFailed` latched first and leaves their physical state unknown
- Give `Error::VerificationFailed` `expected` and `actual` fields, both `Relays`.
  It previously carried nothing, so a caller was told the read-back disagreed but
  not how
- Mark `Error` as `#[non_exhaustive]`, so that future variants can be added
  without another breaking release. Downstream `match`es need a wildcard arm

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
  transfer per line change: one packet of pin states out, one packet of samples
  back, 2 transfers rather than 33. That takes `set_relays` with `Verify::Enabled`
  from 87 transfers to 56 and `self_test()` from 118 to 56, roughly 3.6 ms to
  2.3 ms and 4.8 ms to 2.3 ms at the measured ~41 µs per transfer. The write path
  is deliberately left alone, because the CH341A drives a batched write faster
  than the DATA line settles
- Print CLI errors as `arb: <message>` on stderr, so `arb: no relay board found`
  where it previously said `Error: NotFound`. Exit codes are unchanged
- Render the empty relay set as `none`. `arb --status` previously printed
  `Active relays: ` with nothing after it, which reads as a bug rather than as
  "no relays"

### Fixed

- Put the shift register back on every path out of a read, failures included.
  Reading the A6275 clocks zeros in, so every read writes back what it read; a USB
  error between the two left the register holding zeros while the outputs held
  relays. The documented remedy for a transient error is to retry, and the retried
  read then succeeded and reported "none active" on an energized board. Where the
  contents are genuinely gone the caller now gets `Error::RegisterOutOfSync`
  instead of a transport error that invites exactly that retry
- Restore the shift register when a self-test fails, instead of leaving it holding
  the value that was read back. A failure made the *next* read disagree with the
  latched outputs, on exactly the board already suspected of misreporting. No
  relay moved either way
- Put the *latched* value back into the shift register after a failed
  verification, rather than the value that was read back. The read path is the one
  a mismatch implicates, so leaving its answer in the register made the next read
  agree with the fault instead of with the relays. Reading a board back cannot
  settle what a mismatch left behind either way: the A6275 hands back its shift
  register, not its outputs, and `Error::VerificationFailed` said otherwise
- Raise the USB bulk timeouts to 1000 ms, from 10 ms for reads and 100 ms for
  writes. Ten milliseconds for a USB round trip is tight enough to fail
  spuriously on a loaded host or through a hub, and nothing retries behind it
- Re-attach the kernel driver when the interface is released. The driver was
  previously detached on open and never restored
- Correct a wrong relay bitmask in a documentation example

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

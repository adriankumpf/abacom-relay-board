# abacom-relay-board

A crate for controlling the ABACOM CH341A relay board.

## Getting started

### Requirements

The native [libusb](https://github.com/libusb/libusb) library is required in
order to compile. All systems supported by `libusb` library are also supported.

### Installation

#### As library

Add `arb` as a dependency in Cargo.toml:

```toml
[dependencies]
arb = { git = "https://github.com/adriankumpf/abacom-relay-board", tag = 'v0.8.0' }
```

```rust
use arb::{Relay, Relays, Usb, Verify};

fn main() -> Result<(), arb::Error> {
    // Initialising libusb is the expensive part, so do it once and keep the context
    let usb = Usb::new()?;
    let board = usb.board(None);

    // Activate relays 1 and 3, reading the shift register back to confirm
    board.set_relays(Relay::One | Relay::Three, Verify::Enabled)?;

    // Read the current state
    println!("active: {}", board.relays()?);

    // Turn everything off
    board.set_relays(Relays::NONE, Verify::Enabled)
}
```

A `Usb` is cheap to clone and `Send + Sync`, and a `Board` claims the device only
for the duration of a call, so it never locks another application out of a board
they share. Pass `Some(port)` to `usb.board` to pick one of several boards, or
enumerate them with `usb.boards()`.

These boards report no serial number and no product strings, so where a board is
plugged in is the only thing telling two of them apart — see *Telling boards
apart* in the [API docs](https://docs.rs/arb). Label the cables if the relays
drive anything that must not be actuated by mistake.

#### As binary

Clone the repository and build the binary with `cargo build --features=build-binary --release`.

```console
$ arb --list     # the `1-1.3` notation matches `lsusb -t`
port 1 (1-1)
port 5 (1-5)

$ arb 1 3        # activate relays 1 and 3
$ arb --status
Active relays: 1 3

$ arb 0          # turn everything off
$ arb --status
Active relays: none
```

## References

- [USB-Relaiskarte LRB, 8-fach](https://www.electronic-software-shop.com/hardware/relais/usb-relaiskarte-lrb-8-fach.html)
- [ABACOM: USB-LRB Relaiskarte am Raspberry Pi](https://github.com/olerem/ch341a-relay-board)
- [Project: CH341A relay board](http://forum.abacom-online.de/phpBB3/viewtopic.php?f=51&t=3751)

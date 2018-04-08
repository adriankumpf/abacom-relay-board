# abacom-relay-board

A crate for controlling the ABACOM CH341A relay board.

## Getting started

### Requirements

In order to compile the native [libusb](https://github.com/libusb/libusb) library is required. All systems supported by `libusb` library are also supported.

### Installation

#### As library

Add `arb` as a dependency in Cargo.toml:

```toml
[dependencies]
arb = { git = "https://github.com/adriankumpf/abacom-relay-board", tag = 'v0.2.0' }
```

#### As binary

Clone the repository and build the binary with `cargo build --features=build-binary --release`.

## References

* [USB-Relaiskarte LRB, 8-fach](https://www.electronic-software-shop.com/hardware/relais/usb-relaiskarte-lrb-8-fach.html)
* [ABACOM: USB-LRB Relaiskarte am Raspberry Pi](https://github.com/olerem/ch341a-relay-board)
* [Project: CH341A relay board](http://forum.abacom-online.de/phpBB3/viewtopic.php?f=51&t=3751)

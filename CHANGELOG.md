# Changelog

## [Unreleased]

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

[unreleased]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.5.2...HEAD
[0.5.2]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/adriankumpf/abacom-relay-board/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/adriankumpf/abacom-relay-board/compare/4c44ae3...v0.1.0

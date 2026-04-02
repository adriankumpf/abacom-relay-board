use thiserror::Error;

/// A result type that defaults to the unit type for convenience.
pub type Result<T = ()> = std::result::Result<T, Error>;

/// Errors returned by the `arb` library.
#[derive(Error, Debug)]
pub enum Error {
    /// USB communication error (from `rusb`).
    #[error("{0}")]
    Usb(#[from] rusb::Error),

    /// I/O error (e.g. from writing to stdout in the CLI).
    #[error("{0}")]
    IO(#[from] std::io::Error),

    /// No relay board was found on the USB bus.
    #[error("no relay board found")]
    NotFound,

    /// Multiple relay boards were found and no port was specified to disambiguate.
    #[error("multiple relay boards found")]
    MultipleFound,

    /// The relay state read back after `set_status` did not match the requested state.
    #[error("verification failed")]
    VerificationFailed,

    /// A USB bulk transfer completed with an unexpected length.
    #[error("unexpected usb transfer length: expected {expected} bytes, got {actual}")]
    UnexpectedTransferLength { expected: usize, actual: usize },

    /// The device did not respond as expected (e.g. empty read or failed read-back test).
    #[error("bad device")]
    BadDevice,
}

use thiserror::Error;

/// A result type for the `arb` library.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the `arb` library.
///
/// Non-exhaustive: match with a wildcard arm, so that new variants can be added
/// without breaking callers.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// USB communication error (from `rusb`).
    #[error("{0}")]
    Usb(#[from] rusb::Error),

    /// No relay board was found on the USB bus.
    #[error("no relay board found")]
    NotFound,

    /// Multiple relay boards were found and no port was specified to disambiguate.
    #[error("multiple relay boards found")]
    MultipleFound,

    /// The relay state read back after `set_relays` did not match the requested state.
    #[error("verification failed")]
    VerificationFailed,

    /// A relay number outside the 1–8 range the board provides.
    #[error("invalid relay: expected a number between 1 and 8, got {0}")]
    InvalidRelay(u8),

    /// A USB bulk transfer completed with an unexpected length.
    #[error("unexpected usb transfer length: expected {expected} bytes, got {actual}")]
    UnexpectedTransferLength { expected: usize, actual: usize },

    /// The device did not respond as expected (e.g. empty read or failed read-back test).
    #[error("bad device")]
    BadDevice,
}

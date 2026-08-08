use thiserror::Error;

use crate::relays::Relays;

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
    ///
    /// The relays were latched before the read-back, so the physical relay state is
    /// unknown: `expected`, `actual` or neither. Read the board back to find out.
    ///
    /// Rendered with the sets' `Debug` rather than their `Display`, so that "no
    /// relays" reads as `{}` instead of as nothing at all.
    #[error("verification failed: expected {expected:?}, read back {actual:?}")]
    VerificationFailed {
        /// The relays that were requested.
        expected: Relays,
        /// The relays the shift register reported afterwards.
        actual: Relays,
    },

    /// A relay number outside the 1–8 range the board provides.
    #[error("invalid relay: expected a number between 1 and 8, got {0}")]
    InvalidRelay(u8),

    /// A USB bulk transfer completed with an unexpected length.
    #[error("unexpected usb transfer length: expected {expected} bytes, got {actual}")]
    UnexpectedTransferLength { expected: usize, actual: usize },

    /// The board's self-test failed: a test pattern did not survive the round trip
    /// through the shift register.
    ///
    /// The pattern is written without latching, so the relays were never touched
    /// and their physical state is unchanged — only the read path is suspect. This
    /// is the difference from [`Error::VerificationFailed`], where the relays were
    /// latched before the disagreement was noticed.
    #[error("self-test failed")]
    SelfTestFailed,
}

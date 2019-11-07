use thiserror::Error;

/// A result of a function that may return an `Error`.
pub type Result<T = ()> = std::result::Result<T, Error>;

/// Errors returned by the `arb` library.
#[derive(Error, Debug)]
pub enum Error {
    /// rusb error
    #[error("{0}")]
    Usb(#[from] rusb::Error),

    /// IO error
    #[error("{0}")]
    IO(#[from] std::io::Error),

    /// relay board not found
    #[error("no relay board found")]
    NotFound,

    /// multiple relay baords found
    #[error("multiple relay boards found")]
    MultipleFound,

    /// Verification failed
    #[error("verification failed")]
    VerificationFailed,

    /// Reading would exceeded the expected buffer size
    #[error("unsafe read")]
    UnsafeRead,

    /// Usb device malfunction
    #[error("bad device")]
    BadDevice,
}

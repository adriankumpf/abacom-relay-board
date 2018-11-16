use std::error::Error as StdError;
use std::fmt;
use std::result::Result as StdResult;

use libusb;

/// A result of a function that may return an `Error`.
pub type Result<T = ()> = StdResult<T, Error>;

/// Errors returned by the `arb` library.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// libusb error
    Usb(libusb::Error),

    /// relay board not found
    NotFound,

    /// multiple relay baords found
    MultipleFound,

    /// Verification failed
    VerificationFailed,

    /// Reading would exceeded the expected buffer size
    UnsafeRead,

    /// Usb device malfunction
    BadDevice,
}

impl Error {
    pub fn strerror(&self) -> &str {
        match self {
            Error::Usb(err) => err.description(),
            Error::NotFound => "no relay board found",
            Error::MultipleFound => "multiple relay boards found",
            Error::VerificationFailed => "verification failed",
            Error::UnsafeRead => "unsafe read",
            Error::BadDevice => "bad device",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> StdResult<(), fmt::Error> {
        fmt.write_str(self.strerror())
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        self.strerror()
    }
}

impl From<libusb::Error> for Error {
    fn from(err: libusb::Error) -> Error {
        Error::Usb(err)
    }
}

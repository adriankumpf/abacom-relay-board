use std::fmt;
use std::error::Error as StdError;
use std::result::Result as StdResult;

use libusb;

/// A result of a function that may return a `Error`.
pub type Result<T> = StdResult<T, Error>;

/// Errors returned by the `abacom_relay_board` library.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// Libusb Error
    Usb(libusb::Error),

    /// relay board not found
    NotFound,

    /// Multiple relay baords found
    MultipleFound,
}

impl Error {
    pub fn strerror(&self) -> &str {
        match *self {
            Error::Usb(ref err) => err.description(),
            Error::NotFound => "no relay board found",
            Error::MultipleFound => "multiple relay boards found",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> StdResult<(), fmt::Error> {
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

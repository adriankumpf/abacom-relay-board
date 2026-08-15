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

    /// More than one board answered to the one that was named.
    ///
    /// Either no port was given and several boards are attached, or a port *was*
    /// given and two boards share it: a port number is the port on the parent hub,
    /// so two hubs can both have a board on port 3. Giving the port again is no
    /// remedy in that second case: [`Usb::boards`](crate::Usb::boards) names each
    /// board by its whole path, which cannot collide.
    #[error("multiple relay boards found")]
    MultipleFound,

    /// Another application holds the board's USB interface.
    ///
    /// Expected rather than exceptional: the interface is claimed exclusively, but
    /// only for the duration of a single call, so a board shared between
    /// applications produces this whenever two calls overlap. Unlike
    /// [`Error::Usb`], it says nothing is wrong with the board; retrying is the
    /// remedy.
    #[error("the relay board is in use by another application")]
    Busy,

    /// The relay state read back after `set_relays` did not match the requested state.
    ///
    /// The relays were latched before the read-back, so the physical relay state is
    /// unknown: `expected`, `actual` or neither. Read the board back to find out.
    #[error("verification failed: expected {expected}, read back {actual}")]
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

    /// [`Board::self_test`](crate::Board::self_test) failed: a test pattern did not
    /// survive the round trip through the shift register.
    ///
    /// The pattern is written without latching, so the relays were never touched
    /// and only the read path is suspect. That is the difference from
    /// [`Error::VerificationFailed`], which leaves their physical state unknown.
    #[error("self-test failed")]
    SelfTestFailed,

    /// A read left the shift register no longer holding what the relays hold.
    ///
    /// Reading the A6275 is destructive, since zeros shift in as the contents shift
    /// out, so every read writes back what it read. This is that round trip coming
    /// apart: either the read itself failed, taking contents with it that nothing
    /// can recover, or the write back did.
    ///
    /// The failure moves no relay of its own: it is the register that is lost, not
    /// the outputs. After [`relays`](crate::Board::relays) or
    /// [`self_test`](crate::Board::self_test) the relays hold whatever they already
    /// held; after [`set_relays`](crate::Board::set_relays) they hold the value that
    /// was latched, which is the one the restore was trying to put back.
    ///
    /// What is lost is the board's *account* of them, and that account is what later
    /// reads report: a following `relays` can succeed and report relays as inactive
    /// while they are physically energized. Retrying the read is therefore the one
    /// thing that does not help.
    ///
    /// `set_relays` is the way out: it latches what it writes and leaves the register
    /// holding it, so the two agree again.
    #[error("the shift register is out of sync with the relays: {source}")]
    RegisterOutOfSync {
        /// The transport failure that interrupted the read or the write back.
        source: Box<Error>,
    },
}

impl Error {
    /// Reports `source` as having left the shift register out of sync.
    ///
    /// Conservative on purpose: a stream that failed on its way out may never have
    /// clocked the register, but nothing can tell that from a response that was
    /// lost, and assuming the contents survived is the assumption that lies.
    pub(crate) fn out_of_sync(source: Error) -> Self {
        Error::RegisterOutOfSync {
            source: Box::new(source),
        }
    }
}

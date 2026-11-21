use core::fmt;
use core::num::TryFromIntError;

use bdk_chain::bitcoin;
use bdk_chain::miniscript;
use bitcoin::{consensus, hex::error::HexToArrayError, network::ParseNetworkError};

/// Crate error.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// `bitcoin` consensus encoding error.
    Decode(consensus::encode::Error),
    /// Error converting an integer.
    FromInt(TryFromIntError),
    /// `bitcoin` hex to array error.
    HexToArray(HexToArrayError),
    /// Database migration error.
    Migrate(MigrateError),
    /// `miniscript` error.
    Miniscript(miniscript::Error),
    /// Parse `Network` error.
    ParseNetwork(ParseNetworkError),
    /// `sqlite-thin` error.
    Sqlite(sqlite_thin::Error),
    /// The async worker thread is no longer running.
    #[cfg(feature = "async")]
    WorkerGone,
    /// Failed to start the async worker thread.
    #[cfg(feature = "async")]
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "{e}"),
            Self::FromInt(e) => write!(f, "{e}"),
            Self::HexToArray(e) => write!(f, "{e}"),
            Self::Migrate(e) => write!(f, "{e}"),
            Self::Miniscript(e) => write!(f, "{e}"),
            Self::ParseNetwork(e) => write!(f, "{e}"),
            Self::Sqlite(e) => write!(f, "{e}"),
            #[cfg(feature = "async")]
            Self::WorkerGone => write!(f, "the database worker thread is no longer running"),
            #[cfg(feature = "async")]
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Errors that can occur while running schema migrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MigrateError {
    /// A previously applied migration was not recorded as successful.
    Dirty(i64),
    /// The database contains a migration version this build does not know about.
    VersionMissing(i64),
    /// A migration's contents changed after it was applied.
    VersionMismatch(i64),
}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dirty(version) => write!(
                f,
                "migration {version} is marked as unsuccessful; fix it and remove its row from the migrations table"
            ),
            Self::VersionMissing(version) => write!(
                f,
                "migration {version} is applied but is missing from this build"
            ),
            Self::VersionMismatch(version) => {
                write!(f, "migration {version} has changed since it was applied")
            }
        }
    }
}

impl std::error::Error for MigrateError {}

macro_rules! impl_error_from {
    ( $from:ty, $to:ident ) => {
        impl core::convert::From<$from> for Error {
            fn from(err: $from) -> Self {
                Error::$to(err)
            }
        }
    };
}

impl_error_from!(consensus::encode::Error, Decode);
impl_error_from!(TryFromIntError, FromInt);
impl_error_from!(HexToArrayError, HexToArray);
impl_error_from!(miniscript::Error, Miniscript);
impl_error_from!(MigrateError, Migrate);
impl_error_from!(ParseNetworkError, ParseNetwork);
impl_error_from!(sqlite_thin::Error, Sqlite);

#[cfg(feature = "async")]
impl_error_from!(std::io::Error, Io);

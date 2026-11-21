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
    /// Database error.
    Database(DatabaseError),
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
            Self::Database(e) => write!(f, "{e}"),
            #[cfg(feature = "async")]
            Self::WorkerGone => write!(f, "the database worker thread is no longer running"),
            #[cfg(feature = "async")]
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// An error reported by the database backend.
///
/// The underlying error type is intentionally opaque so that the backend can
/// change without affecting this crate's public API.
#[derive(Debug)]
pub struct DatabaseError(sqlite_thin::Error);

impl DatabaseError {
    #[cfg(any(feature = "blocking", feature = "async"))]
    pub(crate) fn new(error: sqlite_thin::Error) -> Self {
        Self(error)
    }
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

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

/// Builds a public [`Error::Database`] from a raw backend error.
#[cfg(any(feature = "blocking", feature = "async"))]
pub(crate) fn database(error: sqlite_thin::Error) -> Error {
    Error::Database(DatabaseError::new(error))
}

/// Internal error type that may name backend types.
///
/// Only the private `db`, `wallet`, and `migrate` modules use this type.
/// Public methods convert it with [`InternalError::into_public`].
#[cfg(any(feature = "blocking", feature = "async"))]
#[derive(Debug)]
pub(crate) enum InternalError {
    Decode(consensus::encode::Error),
    FromInt(TryFromIntError),
    HexToArray(HexToArrayError),
    Migrate(MigrateError),
    Miniscript(miniscript::Error),
    ParseNetwork(ParseNetworkError),
    Sqlite(sqlite_thin::Error),
}

#[cfg(any(feature = "blocking", feature = "async"))]
impl InternalError {
    pub(crate) fn into_public(self) -> Error {
        match self {
            Self::Decode(e) => Error::Decode(e),
            Self::FromInt(e) => Error::FromInt(e),
            Self::HexToArray(e) => Error::HexToArray(e),
            Self::Migrate(e) => Error::Migrate(e),
            Self::Miniscript(e) => Error::Miniscript(e),
            Self::ParseNetwork(e) => Error::ParseNetwork(e),
            Self::Sqlite(e) => Error::Database(DatabaseError::new(e)),
        }
    }
}

#[cfg(any(feature = "blocking", feature = "async"))]
impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "{e}"),
            Self::FromInt(e) => write!(f, "{e}"),
            Self::HexToArray(e) => write!(f, "{e}"),
            Self::Migrate(e) => write!(f, "{e}"),
            Self::Miniscript(e) => write!(f, "{e}"),
            Self::ParseNetwork(e) => write!(f, "{e}"),
            Self::Sqlite(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(any(feature = "blocking", feature = "async"))]
impl std::error::Error for InternalError {}

macro_rules! impl_error_from {
    ( $from:ty, $to:ident ) => {
        impl core::convert::From<$from> for Error {
            fn from(err: $from) -> Self {
                Error::$to(err)
            }
        }

        #[cfg(any(feature = "blocking", feature = "async"))]
        impl core::convert::From<$from> for InternalError {
            fn from(err: $from) -> Self {
                InternalError::$to(err)
            }
        }
    };
}

macro_rules! impl_internal_error_from {
    ( $from:ty, $to:ident ) => {
        #[cfg(any(feature = "blocking", feature = "async"))]
        impl core::convert::From<$from> for InternalError {
            fn from(err: $from) -> Self {
                InternalError::$to(err)
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
impl_internal_error_from!(sqlite_thin::Error, Sqlite);

#[cfg(feature = "async")]
impl core::convert::From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

#[cfg(any(feature = "blocking", feature = "async"))]
mod db;
mod error;
pub use error::*;
#[cfg(any(feature = "blocking", feature = "async"))]
mod migrate;
#[cfg(feature = "blocking")]
mod store;
#[cfg(all(feature = "wallet", any(feature = "blocking", feature = "async")))]
mod wallet;
#[cfg(feature = "blocking")]
pub use store::*;
#[cfg(feature = "async")]
mod async_store;
#[cfg(feature = "async")]
pub use async_store::*;

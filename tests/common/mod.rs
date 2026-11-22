//! Test helpers vendored from BDK's `bdk_testenv` crate.
//!
//! `bdk_testenv`'s `persist_test_utils` is not published yet, so the harness and
//! the helpers it needs are vendored here from
//! <https://github.com/bitcoindevkit/bdk/tree/master/crates/testenv>.
//! Licensed under MIT OR Apache-2.0. Remove this directory and switch to the
//! crates.io `bdk_testenv` dependency once `persist_test_utils` is released.
//!
//! The only changes from upstream are import paths and removal of the
//! `miniscript` feature gates, because `bdk_sqlite` always enables that feature
//! on `bdk_chain`.

pub mod persist_test_utils;
pub mod utils;

//! Blocking [`Store`] implementation.

#[cfg(feature = "wallet")]
use std::collections::BTreeMap;

use bdk_chain::{ConfirmationBlockTime, keychain_txout, local_chain, tx_graph};
#[cfg(feature = "wallet")]
use bdk_chain::{bitcoin, miniscript};
#[cfg(feature = "wallet")]
use bdk_wallet::{ChangeSet, KeychainKind, WalletPersister, locked_outpoints};
#[cfg(feature = "wallet")]
use bitcoin::Network;
#[cfg(feature = "wallet")]
use miniscript::descriptor::{Descriptor, DescriptorPublicKey};
use sqlite_thin::Connection;

use crate::db;
use crate::error::{Error, InternalError, database};
#[cfg(feature = "wallet")]
use crate::wallet;

/// Store.
///
/// The blocking store owns a single SQLite connection. It is not cloneable;
/// create one store per database handle.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Create a new [`Store`] instance.
    ///
    /// This will create a new database at the given path if it doesn't exist.
    ///
    /// Note that `path` can be a filename, e.g. `foo.db`, an absolute path, or
    /// a sqlx-style URL such as `sqlite://foo.db`. `:memory:` and
    /// `sqlite::memory:` create an in-memory database.
    pub fn new(path: &str) -> Result<Self, Error> {
        let conn = db::open(path).map_err(InternalError::into_public)?;
        Ok(Self { conn })
    }

    /// New in memory.
    pub fn new_memory() -> Result<Self, Error> {
        let conn = Connection::open_in_memory().map_err(database)?;
        Ok(Self { conn })
    }

    /// Runs pending migrations against the database.
    pub fn migrate(&self) -> Result<(), Error> {
        crate::migrate::migrate(&self.conn).map_err(InternalError::into_public)
    }

    /// Write tx_graph.
    pub fn write_tx_graph(
        &self,
        tx_graph: &tx_graph::ChangeSet<ConfirmationBlockTime>,
    ) -> Result<(), Error> {
        self.with_transaction(|conn| db::write_tx_graph(conn, tx_graph))
    }

    /// Write local_chain.
    pub fn write_local_chain(&self, local_chain: &local_chain::ChangeSet) -> Result<(), Error> {
        self.with_transaction(|conn| db::write_local_chain(conn, local_chain))
    }

    /// Write keychain_txout.
    pub fn write_keychain_txout(
        &self,
        keychain_txout: &keychain_txout::ChangeSet,
    ) -> Result<(), Error> {
        self.with_transaction(|conn| db::write_keychain_txout(conn, keychain_txout))
    }

    /// Read tx_graph.
    pub fn read_tx_graph(&self) -> Result<tx_graph::ChangeSet<ConfirmationBlockTime>, Error> {
        db::read_tx_graph(&self.conn).map_err(InternalError::into_public)
    }

    /// Read local_chain.
    pub fn read_local_chain(&self) -> Result<local_chain::ChangeSet, Error> {
        db::read_local_chain(&self.conn).map_err(InternalError::into_public)
    }

    /// Read keychain_txout.
    pub fn read_keychain_txout(&self) -> Result<keychain_txout::ChangeSet, Error> {
        db::read_keychain_txout(&self.conn).map_err(InternalError::into_public)
    }

    fn with_transaction<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, InternalError>,
    ) -> Result<T, Error> {
        db::with_transaction(&self.conn, f).map_err(InternalError::into_public)
    }
}

#[cfg(feature = "wallet")]
impl Store {
    /// Write changeset.
    pub fn write_changeset(&self, changeset: &ChangeSet) -> Result<(), Error> {
        wallet::write_changeset(&self.conn, changeset).map_err(InternalError::into_public)
    }

    /// Write network.
    pub fn write_network(&self, network: Network) -> Result<(), Error> {
        self.with_transaction(|conn| wallet::write_network(conn, network))
    }

    /// Write keychain descriptors.
    pub fn write_keychain_descriptors(
        &self,
        descriptors: &BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>,
    ) -> Result<(), Error> {
        self.with_transaction(|conn| wallet::write_keychain_descriptors(conn, descriptors))
    }

    /// Read changeset.
    pub fn read_changeset(&self) -> Result<ChangeSet, Error> {
        wallet::read_changeset(&self.conn).map_err(InternalError::into_public)
    }

    /// Read network.
    pub fn read_network(&self) -> Result<Option<Network>, Error> {
        wallet::read_network(&self.conn).map_err(InternalError::into_public)
    }

    /// Read keychain descriptors.
    pub fn read_keychain_descriptors(
        &self,
    ) -> Result<BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>, Error> {
        wallet::read_keychain_descriptors(&self.conn).map_err(InternalError::into_public)
    }

    /// Write locked outpoints.
    pub fn write_locked_outpoints(
        &self,
        locked_outpoints: &locked_outpoints::ChangeSet,
    ) -> Result<(), Error> {
        self.with_transaction(|conn| wallet::write_locked_outpoints(conn, locked_outpoints))
    }

    /// Read locked outpoints.
    pub fn read_locked_outpoints(&self) -> Result<locked_outpoints::ChangeSet, Error> {
        wallet::read_locked_outpoints(&self.conn).map_err(InternalError::into_public)
    }
}

#[cfg(feature = "wallet")]
impl WalletPersister for Store {
    type Error = crate::Error;

    fn initialize(persister: &mut Self) -> Result<ChangeSet, Self::Error> {
        persister.migrate()?;
        persister.read_changeset()
    }

    fn persist(persister: &mut Self, changeset: &ChangeSet) -> Result<(), Self::Error> {
        persister.write_changeset(changeset)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bdk_chain::local_chain;
    use bitcoin::{BlockHash, hashes::Hash};

    #[test]
    fn block_table_height_is_unique() -> anyhow::Result<()> {
        let mut cs = local_chain::ChangeSet::default();
        cs.blocks.insert(0, Some(Hash::hash(b"0")));
        cs.blocks.insert(1, Some(Hash::hash(b"1")));

        let store = Store::new_memory()?;
        store.migrate()?;
        store.write_local_chain(&cs)?;

        // Trying to replace the value of existing height should be ignored.
        cs.blocks.insert(1, Some(Hash::hash(b"1a")));
        store.write_local_chain(&cs)?;

        let rows = {
            let mut stmt = store
                .conn
                .prepare("SELECT height, hash FROM block WHERE height = 1")?;
            let mut rows = stmt.query([])?;
            let mut rows_out: Vec<(u32, String)> = Vec::new();
            while let Some(row) = rows.next()? {
                rows_out.push((row.get(0)?, row.get(1)?));
            }
            rows_out
        };

        assert_eq!(rows.len(), 1, "Expected 1 block row");
        let expected_hash: BlockHash = Hash::hash(b"1");
        assert_eq!(rows[0].1, expected_hash.to_string());

        // Delete row 1 and insert hash "1a" again.
        let mut cs = local_chain::ChangeSet::default();
        cs.blocks.insert(1, None);
        store.write_local_chain(&cs)?;

        cs.blocks.insert(1, Some(Hash::hash(b"1a")));
        store.write_local_chain(&cs)?;

        let rows = {
            let mut stmt = store
                .conn
                .prepare("SELECT height, hash FROM block WHERE height = 1")?;
            let mut rows = stmt.query([])?;
            let mut rows_out: Vec<(u32, String)> = Vec::new();
            while let Some(row) = rows.next()? {
                rows_out.push((row.get(0)?, row.get(1)?));
            }
            rows_out
        };

        // Row hash should change to "1a".
        assert_eq!(rows.len(), 1, "Expected 1 block row");
        let expected_hash: BlockHash = Hash::hash(b"1a");
        assert_eq!(rows[0].1, expected_hash.to_string());

        Ok(())
    }

    #[cfg(feature = "wallet")]
    #[test]
    fn wallet_changeset_round_trip() -> anyhow::Result<()> {
        use bitcoin::{OutPoint, Txid};

        let store = Store::new_memory()?;
        store.migrate()?;

        let mut cs = ChangeSet {
            network: Some(Network::Bitcoin),
            ..Default::default()
        };
        cs.local_chain.blocks.insert(0, Some(Hash::hash(b"0")));
        cs.locked_outpoints.outpoints.insert(
            OutPoint {
                txid: Txid::all_zeros(),
                vout: 1,
            },
            true,
        );

        store.write_changeset(&cs)?;
        let read = store.read_changeset()?;

        assert_eq!(read.network, Some(Network::Bitcoin));
        assert_eq!(read.local_chain, cs.local_chain);
        assert_eq!(read.locked_outpoints, cs.locked_outpoints);

        Ok(())
    }
}

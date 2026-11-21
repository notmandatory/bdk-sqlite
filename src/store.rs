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
use crate::error::Error;
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
        let conn = db::open(path)?;
        Ok(Self { conn })
    }

    /// New in memory.
    pub fn new_memory() -> Result<Self, Error> {
        let conn = Connection::open_in_memory()?;
        Ok(Self { conn })
    }

    /// Create a new [`Store`] from an existing [`Connection`].
    pub fn from_connection(conn: Connection) -> Self {
        Self { conn }
    }

    /// The underlying SQLite connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Runs pending migrations against the database.
    pub fn migrate(&self) -> Result<(), Error> {
        crate::migrate::migrate(&self.conn)
    }
}

impl Store {
    /// Write tx_graph.
    pub fn write_tx_graph(
        conn: &Connection,
        tx_graph: &tx_graph::ChangeSet<ConfirmationBlockTime>,
    ) -> Result<(), Error> {
        db::write_tx_graph(conn, tx_graph)
    }

    /// Write local_chain.
    pub fn write_local_chain(
        conn: &Connection,
        local_chain: &local_chain::ChangeSet,
    ) -> Result<(), Error> {
        db::write_local_chain(conn, local_chain)
    }

    /// Write keychain_txout.
    pub fn write_keychain_txout(
        conn: &Connection,
        keychain_txout: &keychain_txout::ChangeSet,
    ) -> Result<(), Error> {
        db::write_keychain_txout(conn, keychain_txout)
    }

    /// Read tx_graph.
    pub fn read_tx_graph(
        conn: &Connection,
    ) -> Result<tx_graph::ChangeSet<ConfirmationBlockTime>, Error> {
        db::read_tx_graph(conn)
    }

    /// Read local_chain.
    pub fn read_local_chain(conn: &Connection) -> Result<local_chain::ChangeSet, Error> {
        db::read_local_chain(conn)
    }

    /// Read keychain_txout.
    pub fn read_keychain_txout(conn: &Connection) -> Result<keychain_txout::ChangeSet, Error> {
        db::read_keychain_txout(conn)
    }
}

#[cfg(feature = "wallet")]
impl Store {
    /// Write changeset.
    pub fn write_changeset(&self, changeset: &ChangeSet) -> Result<(), Error> {
        wallet::write_changeset(&self.conn, changeset)
    }

    /// Write network.
    pub fn write_network(conn: &Connection, network: Network) -> Result<(), Error> {
        wallet::write_network(conn, network)
    }

    /// Write keychain descriptors.
    pub fn write_keychain_descriptors(
        conn: &Connection,
        descriptors: &BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>,
    ) -> Result<(), Error> {
        wallet::write_keychain_descriptors(conn, descriptors)
    }

    /// Read changeset.
    pub fn read_changeset(&self) -> Result<ChangeSet, Error> {
        wallet::read_changeset(&self.conn)
    }

    /// Read network.
    pub fn read_network(conn: &Connection) -> Result<Option<Network>, Error> {
        wallet::read_network(conn)
    }

    /// Read keychain descriptors.
    pub fn read_keychain_descriptors(
        conn: &Connection,
    ) -> Result<BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>, Error> {
        wallet::read_keychain_descriptors(conn)
    }

    /// Write locked outpoints.
    pub fn write_locked_outpoints(
        conn: &Connection,
        locked_outpoints: &locked_outpoints::ChangeSet,
    ) -> Result<(), Error> {
        wallet::write_locked_outpoints(conn, locked_outpoints)
    }

    /// Read locked outpoints.
    pub fn read_locked_outpoints(conn: &Connection) -> Result<locked_outpoints::ChangeSet, Error> {
        wallet::read_locked_outpoints(conn)
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

        {
            let txn = store.connection().unchecked_transaction()?;
            Store::write_local_chain(&txn, &cs).expect("failed to write `local_chain`");
            txn.commit()?;
        }

        // Trying to replace the value of existing height should be ignored.
        cs.blocks.insert(1, Some(Hash::hash(b"1a")));

        {
            let txn = store.connection().unchecked_transaction()?;
            Store::write_local_chain(&txn, &cs).expect("failed to write `local_chain`");
            txn.commit()?;
        }

        let rows = {
            let mut stmt = store
                .connection()
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

        {
            let txn = store.connection().unchecked_transaction()?;
            Store::write_local_chain(&txn, &cs).expect("failed to write `local_chain`");
            txn.commit()?;
        }

        cs.blocks.insert(1, Some(Hash::hash(b"1a")));

        {
            let txn = store.connection().unchecked_transaction()?;
            Store::write_local_chain(&txn, &cs).expect("failed to write `local_chain`");
            txn.commit()?;
        }

        let rows = {
            let mut stmt = store
                .connection()
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

//! Async [`AsyncStore`] implementation backed by a dedicated worker thread.
//!
//! The worker owns the single SQLite connection. Callers await replies over a
//! runtime-agnostic oneshot channel, so no particular async runtime is needed.

#[cfg(feature = "wallet")]
use std::collections::BTreeMap;
#[cfg(feature = "wallet")]
use std::future::Future;
#[cfg(feature = "wallet")]
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use bdk_chain::{ConfirmationBlockTime, keychain_txout, local_chain, tx_graph};
#[cfg(feature = "wallet")]
use bdk_chain::{bitcoin, miniscript};
#[cfg(feature = "wallet")]
use bdk_wallet::{AsyncWalletPersister, ChangeSet, KeychainKind, locked_outpoints};
#[cfg(feature = "wallet")]
use bitcoin::Network;
use futures_channel::oneshot;
#[cfg(feature = "wallet")]
use miniscript::descriptor::{Descriptor, DescriptorPublicKey};
use sqlite_thin::Connection;

use crate::error::{Error, InternalError, database};
#[cfg(feature = "wallet")]
use crate::wallet;
use crate::{db, migrate};

type Reply<T> = oneshot::Sender<Result<T, Error>>;

enum Command {
    Migrate(Reply<()>),
    ReadTxGraph(Reply<tx_graph::ChangeSet<ConfirmationBlockTime>>),
    WriteTxGraph(Box<tx_graph::ChangeSet<ConfirmationBlockTime>>, Reply<()>),
    ReadLocalChain(Reply<local_chain::ChangeSet>),
    WriteLocalChain(Box<local_chain::ChangeSet>, Reply<()>),
    ReadKeychainTxout(Reply<keychain_txout::ChangeSet>),
    WriteKeychainTxout(Box<keychain_txout::ChangeSet>, Reply<()>),
    #[cfg(feature = "wallet")]
    ReadChangeset(Reply<ChangeSet>),
    #[cfg(feature = "wallet")]
    WriteChangeset(Box<ChangeSet>, Reply<()>),
    #[cfg(feature = "wallet")]
    ReadNetwork(Reply<Option<Network>>),
    #[cfg(feature = "wallet")]
    WriteNetwork(Network, Reply<()>),
    #[cfg(feature = "wallet")]
    ReadKeychainDescriptors(Reply<BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>>),
    #[cfg(feature = "wallet")]
    WriteKeychainDescriptors(
        BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>,
        Reply<()>,
    ),
    #[cfg(feature = "wallet")]
    ReadLockedOutpoints(Reply<locked_outpoints::ChangeSet>),
    #[cfg(feature = "wallet")]
    WriteLockedOutpoints(Box<locked_outpoints::ChangeSet>, Reply<()>),
}

/// Async store.
///
/// Cloning is cheap: every clone shares the same worker thread and database
/// connection. When the last clone is dropped, the worker exits.
#[derive(Debug, Clone)]
pub struct AsyncStore {
    tx: Sender<Command>,
}

impl AsyncStore {
    /// Create a new [`AsyncStore`].
    ///
    /// This will create a new database at the given path if it doesn't exist.
    /// See the blocking `Store::new` for the accepted path formats.
    pub async fn new(path: &str) -> Result<Self, Error> {
        let conn = db::open(path).map_err(InternalError::into_public)?;
        Self::spawn(conn)
    }

    /// New in memory.
    pub async fn new_memory() -> Result<Self, Error> {
        let conn = Connection::open_in_memory().map_err(database)?;
        Self::spawn(conn)
    }

    fn spawn(conn: Connection) -> Result<Self, Error> {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("bdk-sqlite".to_string())
            .spawn(move || run(conn, rx))
            .map_err(Error::Io)?;
        Ok(Self { tx })
    }

    /// Runs pending migrations against the database.
    pub async fn migrate(&self) -> Result<(), Error> {
        self.request(Command::Migrate).await
    }

    /// Write tx_graph.
    pub async fn write_tx_graph(
        &self,
        tx_graph: &tx_graph::ChangeSet<ConfirmationBlockTime>,
    ) -> Result<(), Error> {
        self.request(|reply| Command::WriteTxGraph(Box::new(tx_graph.clone()), reply))
            .await
    }

    /// Write local_chain.
    pub async fn write_local_chain(
        &self,
        local_chain: &local_chain::ChangeSet,
    ) -> Result<(), Error> {
        self.request(|reply| Command::WriteLocalChain(Box::new(local_chain.clone()), reply))
            .await
    }

    /// Write keychain_txout.
    pub async fn write_keychain_txout(
        &self,
        keychain_txout: &keychain_txout::ChangeSet,
    ) -> Result<(), Error> {
        self.request(|reply| Command::WriteKeychainTxout(Box::new(keychain_txout.clone()), reply))
            .await
    }

    /// Read tx_graph.
    pub async fn read_tx_graph(&self) -> Result<tx_graph::ChangeSet<ConfirmationBlockTime>, Error> {
        self.request(Command::ReadTxGraph).await
    }

    /// Read local_chain.
    pub async fn read_local_chain(&self) -> Result<local_chain::ChangeSet, Error> {
        self.request(Command::ReadLocalChain).await
    }

    /// Read keychain_txout.
    pub async fn read_keychain_txout(&self) -> Result<keychain_txout::ChangeSet, Error> {
        self.request(Command::ReadKeychainTxout).await
    }

    #[cfg(feature = "wallet")]
    /// Write changeset.
    pub async fn write_changeset(&self, changeset: &ChangeSet) -> Result<(), Error> {
        self.request(|reply| Command::WriteChangeset(Box::new(changeset.clone()), reply))
            .await
    }

    #[cfg(feature = "wallet")]
    /// Write network.
    pub async fn write_network(&self, network: Network) -> Result<(), Error> {
        self.request(|reply| Command::WriteNetwork(network, reply))
            .await
    }

    #[cfg(feature = "wallet")]
    /// Write keychain descriptors.
    pub async fn write_keychain_descriptors(
        &self,
        descriptors: &BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>,
    ) -> Result<(), Error> {
        self.request(|reply| Command::WriteKeychainDescriptors(descriptors.clone(), reply))
            .await
    }

    #[cfg(feature = "wallet")]
    /// Read changeset.
    pub async fn read_changeset(&self) -> Result<ChangeSet, Error> {
        self.request(Command::ReadChangeset).await
    }

    #[cfg(feature = "wallet")]
    /// Read network.
    pub async fn read_network(&self) -> Result<Option<Network>, Error> {
        self.request(Command::ReadNetwork).await
    }

    #[cfg(feature = "wallet")]
    /// Read keychain descriptors.
    pub async fn read_keychain_descriptors(
        &self,
    ) -> Result<BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>, Error> {
        self.request(Command::ReadKeychainDescriptors).await
    }

    #[cfg(feature = "wallet")]
    /// Write locked outpoints.
    pub async fn write_locked_outpoints(
        &self,
        locked_outpoints: &locked_outpoints::ChangeSet,
    ) -> Result<(), Error> {
        self.request(|reply| {
            Command::WriteLockedOutpoints(Box::new(locked_outpoints.clone()), reply)
        })
        .await
    }

    #[cfg(feature = "wallet")]
    /// Read locked outpoints.
    pub async fn read_locked_outpoints(&self) -> Result<locked_outpoints::ChangeSet, Error> {
        self.request(Command::ReadLockedOutpoints).await
    }

    async fn request<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> Command,
    ) -> Result<T, Error> {
        let (reply, response) = oneshot::channel();
        self.tx.send(make(reply)).map_err(|_| Error::WorkerGone)?;
        response.await.map_err(|_| Error::WorkerGone)?
    }
}

fn public<T>(result: Result<T, InternalError>) -> Result<T, Error> {
    result.map_err(InternalError::into_public)
}

fn run(conn: Connection, rx: Receiver<Command>) {
    while let Ok(command) = rx.recv() {
        match command {
            Command::Migrate(reply) => {
                let _ = reply.send(public(migrate::migrate(&conn)));
            }
            Command::ReadTxGraph(reply) => {
                let _ = reply.send(public(db::read_tx_graph(&conn)));
            }
            Command::WriteTxGraph(changeset, reply) => {
                let _ = reply.send(public(db::with_transaction(&conn, |conn| {
                    db::write_tx_graph(conn, &changeset)
                })));
            }
            Command::ReadLocalChain(reply) => {
                let _ = reply.send(public(db::read_local_chain(&conn)));
            }
            Command::WriteLocalChain(changeset, reply) => {
                let _ = reply.send(public(db::with_transaction(&conn, |conn| {
                    db::write_local_chain(conn, &changeset)
                })));
            }
            Command::ReadKeychainTxout(reply) => {
                let _ = reply.send(public(db::read_keychain_txout(&conn)));
            }
            Command::WriteKeychainTxout(changeset, reply) => {
                let _ = reply.send(public(db::with_transaction(&conn, |conn| {
                    db::write_keychain_txout(conn, &changeset)
                })));
            }
            #[cfg(feature = "wallet")]
            Command::ReadChangeset(reply) => {
                let _ = reply.send(public(wallet::read_changeset(&conn)));
            }
            #[cfg(feature = "wallet")]
            Command::WriteChangeset(changeset, reply) => {
                let _ = reply.send(public(wallet::write_changeset(&conn, &changeset)));
            }
            #[cfg(feature = "wallet")]
            Command::ReadNetwork(reply) => {
                let _ = reply.send(public(wallet::read_network(&conn)));
            }
            #[cfg(feature = "wallet")]
            Command::WriteNetwork(network, reply) => {
                let _ = reply.send(public(db::with_transaction(&conn, |conn| {
                    wallet::write_network(conn, network)
                })));
            }
            #[cfg(feature = "wallet")]
            Command::ReadKeychainDescriptors(reply) => {
                let _ = reply.send(public(wallet::read_keychain_descriptors(&conn)));
            }
            #[cfg(feature = "wallet")]
            Command::WriteKeychainDescriptors(descriptors, reply) => {
                let _ = reply.send(public(db::with_transaction(&conn, |conn| {
                    wallet::write_keychain_descriptors(conn, &descriptors)
                })));
            }
            #[cfg(feature = "wallet")]
            Command::ReadLockedOutpoints(reply) => {
                let _ = reply.send(public(wallet::read_locked_outpoints(&conn)));
            }
            #[cfg(feature = "wallet")]
            Command::WriteLockedOutpoints(locked_outpoints, reply) => {
                let _ = reply.send(public(db::with_transaction(&conn, |conn| {
                    wallet::write_locked_outpoints(conn, &locked_outpoints)
                })));
            }
        }
    }
}

#[cfg(feature = "wallet")]
type FutureResult<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + 'a + Send>>;

#[cfg(feature = "wallet")]
impl AsyncWalletPersister for AsyncStore {
    type Error = crate::Error;

    fn initialize<'a>(persister: &'a mut Self) -> FutureResult<'a, ChangeSet, Self::Error>
    where
        Self: 'a,
    {
        Box::pin(async {
            persister.migrate().await?;
            persister.read_changeset().await
        })
    }

    fn persist<'a>(
        persister: &'a mut Self,
        changeset: &'a ChangeSet,
    ) -> FutureResult<'a, (), Self::Error>
    where
        Self: 'a,
    {
        Box::pin(async { persister.write_changeset(changeset).await })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bdk_chain::local_chain;
    use bitcoin::hashes::Hash;

    #[tokio::test]
    async fn local_chain_round_trip() -> anyhow::Result<()> {
        let store = AsyncStore::new_memory().await?;
        store.migrate().await?;

        let mut cs = local_chain::ChangeSet::default();
        cs.blocks.insert(0, Some(Hash::hash(b"0")));

        store.write_local_chain(&cs).await?;
        let read = store.read_local_chain().await?;
        assert_eq!(read, cs);

        Ok(())
    }

    #[cfg(feature = "wallet")]
    #[tokio::test]
    async fn wallet_changeset_round_trip() -> anyhow::Result<()> {
        use bitcoin::{OutPoint, Txid};

        let store = AsyncStore::new_memory().await?;
        store.migrate().await?;

        let mut cs = ChangeSet {
            network: Some(Network::Bitcoin),
            ..Default::default()
        };
        cs.locked_outpoints.outpoints.insert(
            OutPoint {
                txid: Txid::all_zeros(),
                vout: 0,
            },
            true,
        );

        store.write_changeset(&cs).await?;
        let read = store.read_changeset().await?;

        assert_eq!(read.network, Some(Network::Bitcoin));
        assert_eq!(read.locked_outpoints, cs.locked_outpoints);

        Ok(())
    }
}

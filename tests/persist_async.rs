#![cfg(feature = "async")]

mod common;

use std::path::Path;

use bdk_sqlite::AsyncStore;
#[cfg(feature = "wallet")]
use bdk_wallet::persist_test_utils::{
    persist_keychains, persist_network, persist_single_keychain, persist_wallet_changeset,
};
#[cfg(feature = "wallet")]
use bdk_wallet::{AsyncWalletPersister, ChangeSet, WalletPersister};
use common::persist_test_utils::{
    assert_persist_changesets, keychain_txout_changesets, local_chain_changesets,
    tx_graph_changesets,
};

/// A blocking [`WalletPersister`] adapter around [`AsyncStore`] so the test
/// harnesses, which only know about the blocking trait, can drive it.
struct TestStore {
    store: AsyncStore,
    rt: tokio::runtime::Runtime,
}

impl TestStore {
    fn open(path: &Path) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Runtime::new()?;
        let store = rt.block_on(AsyncStore::new(path.to_str().unwrap()))?;
        rt.block_on(store.migrate())?;
        Ok(Self { store, rt })
    }
}

#[cfg(feature = "wallet")]
impl WalletPersister for TestStore {
    type Error = bdk_sqlite::Error;

    fn initialize(persister: &mut Self) -> Result<ChangeSet, Self::Error> {
        persister
            .rt
            .block_on(AsyncWalletPersister::initialize(&mut persister.store))
    }

    fn persist(persister: &mut Self, changeset: &ChangeSet) -> Result<(), Self::Error> {
        persister.rt.block_on(AsyncWalletPersister::persist(
            &mut persister.store,
            changeset,
        ))
    }
}

#[test]
fn tx_graph_changesets_are_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tx_graph.db");

    assert_persist_changesets(
        || Ok(TestStore::open(&path)?),
        |store| Ok(store.rt.block_on(store.store.read_tx_graph())?),
        |store, changeset| Ok(store.rt.block_on(store.store.write_tx_graph(changeset))?),
        &tx_graph_changesets(),
    );
}

#[test]
fn local_chain_changesets_are_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local_chain.db");

    assert_persist_changesets(
        || Ok(TestStore::open(&path)?),
        |store| Ok(store.rt.block_on(store.store.read_local_chain())?),
        |store, changeset| {
            Ok(store
                .rt
                .block_on(store.store.write_local_chain(changeset))?)
        },
        &local_chain_changesets(),
    );
}

#[test]
fn keychain_txout_changesets_are_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keychain_txout.db");

    assert_persist_changesets(
        || Ok(TestStore::open(&path)?),
        |store| Ok(store.rt.block_on(store.store.read_keychain_txout())?),
        |store, changeset| {
            Ok(store
                .rt
                .block_on(store.store.write_keychain_txout(changeset))?)
        },
        &keychain_txout_changesets(),
    );
}

#[cfg(feature = "wallet")]
#[test]
fn wallet_changesets_are_persisted() {
    persist_wallet_changeset("wallet.db", TestStore::open);
}

#[cfg(feature = "wallet")]
#[test]
fn network_is_persisted() {
    persist_network("wallet.db", TestStore::open);
}

#[cfg(feature = "wallet")]
#[test]
fn keychains_are_persisted() {
    persist_keychains("wallet.db", TestStore::open);
}

#[cfg(feature = "wallet")]
#[test]
fn single_keychain_is_persisted() {
    persist_single_keychain("wallet.db", TestStore::open);
}

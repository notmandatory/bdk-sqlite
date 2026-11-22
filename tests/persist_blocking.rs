#![cfg(feature = "blocking")]

mod common;

use std::path::Path;

use bdk_sqlite::Store;
#[cfg(feature = "wallet")]
use bdk_wallet::persist_test_utils::{
    persist_keychains, persist_network, persist_single_keychain, persist_wallet_changeset,
};
use common::persist_test_utils::{
    assert_persist_changesets, keychain_txout_changesets, local_chain_changesets,
    tx_graph_changesets,
};

fn open(path: &Path) -> anyhow::Result<Store> {
    let store = Store::new(path.to_str().unwrap())?;
    store.migrate()?;
    Ok(store)
}

#[test]
fn tx_graph_changesets_are_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tx_graph.db");

    assert_persist_changesets(
        || Ok(open(&path)?),
        |store| Ok(store.read_tx_graph()?),
        |store, changeset| Ok(store.write_tx_graph(changeset)?),
        &tx_graph_changesets(),
    );
}

#[test]
fn local_chain_changesets_are_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local_chain.db");

    assert_persist_changesets(
        || Ok(open(&path)?),
        |store| Ok(store.read_local_chain()?),
        |store, changeset| Ok(store.write_local_chain(changeset)?),
        &local_chain_changesets(),
    );
}

#[test]
fn keychain_txout_changesets_are_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keychain_txout.db");

    assert_persist_changesets(
        || Ok(open(&path)?),
        |store| Ok(store.read_keychain_txout()?),
        |store, changeset| Ok(store.write_keychain_txout(changeset)?),
        &keychain_txout_changesets(),
    );
}

#[cfg(feature = "wallet")]
#[test]
fn wallet_changesets_are_persisted() {
    persist_wallet_changeset("wallet.db", open);
}

#[cfg(feature = "wallet")]
#[test]
fn network_is_persisted() {
    persist_network("wallet.db", open);
}

#[cfg(feature = "wallet")]
#[test]
fn keychains_are_persisted() {
    persist_keychains("wallet.db", open);
}

#[cfg(feature = "wallet")]
#[test]
fn single_keychain_is_persisted() {
    persist_single_keychain("wallet.db", open);
}

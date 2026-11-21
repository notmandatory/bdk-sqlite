//! Change set persistence shared by the blocking and async stores.

use std::collections::BTreeMap;
use std::str::FromStr;

use bdk_chain::bitcoin;
use bdk_chain::miniscript;
use bdk_wallet::{ChangeSet, KeychainKind, locked_outpoints};
use bitcoin::Network;
use bitcoin::OutPoint;
use miniscript::descriptor::{Descriptor, DescriptorPublicKey};
use sqlite_thin::{Connection, OptionalExtension, params};

use crate::db;
use crate::error::Error;

/// Write changeset.
pub(crate) fn write_changeset(conn: &Connection, changeset: &ChangeSet) -> Result<(), Error> {
    let txn = conn.unchecked_transaction()?;

    if let Some(network) = changeset.network {
        write_network(&txn, network)?;
    }

    let mut descriptors = BTreeMap::new();
    if let Some(ref descriptor) = changeset.descriptor {
        descriptors.insert(KeychainKind::External, descriptor.clone());
    }
    if let Some(ref change_descriptor) = changeset.change_descriptor {
        descriptors.insert(KeychainKind::Internal, change_descriptor.clone());
    }
    write_keychain_descriptors(&txn, &descriptors)?;

    db::write_local_chain(&txn, &changeset.local_chain)?;
    db::write_tx_graph(&txn, &changeset.tx_graph)?;
    db::write_keychain_txout(&txn, &changeset.indexer)?;
    write_locked_outpoints(&txn, &changeset.locked_outpoints)?;

    txn.commit()?;
    Ok(())
}

/// Read changeset.
pub(crate) fn read_changeset(conn: &Connection) -> Result<ChangeSet, Error> {
    let txn = conn.unchecked_transaction()?;

    let network = read_network(&txn)?;

    let descriptors = read_keychain_descriptors(&txn)?;
    let descriptor = descriptors.get(&KeychainKind::External).cloned();
    let change_descriptor = descriptors.get(&KeychainKind::Internal).cloned();

    let tx_graph = db::read_tx_graph(&txn)?;
    let local_chain = db::read_local_chain(&txn)?;
    let indexer = db::read_keychain_txout(&txn)?;
    let locked_outpoints = read_locked_outpoints(&txn)?;

    txn.commit()?;
    Ok(ChangeSet {
        network,
        descriptor,
        change_descriptor,
        tx_graph,
        local_chain,
        indexer,
        locked_outpoints,
    })
}

/// Write network.
pub(crate) fn write_network(conn: &Connection, network: Network) -> Result<(), Error> {
    conn.execute(
        "INSERT OR IGNORE INTO network(id, network) VALUES(0, ?1)",
        params![network.to_string()],
    )?;
    Ok(())
}

/// Write keychain descriptors.
pub(crate) fn write_keychain_descriptors(
    conn: &Connection,
    descriptors: &BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>,
) -> Result<(), Error> {
    for (keychain, descriptor) in descriptors {
        let keychain = match keychain {
            KeychainKind::External => 0u8,
            KeychainKind::Internal => 1,
        };
        conn.execute(
            "INSERT OR IGNORE INTO keychain(keychain, descriptor) VALUES(?1, ?2)",
            params![keychain, descriptor.to_string()],
        )?;
    }

    Ok(())
}

/// Read network.
pub(crate) fn read_network(conn: &Connection) -> Result<Option<Network>, Error> {
    let network: Option<String> = conn
        .query_row("SELECT network FROM network", [], |row| row.get(0))
        .optional()?;

    network
        .map(|network| network.parse().map_err(Error::ParseNetwork))
        .transpose()
}

/// Read keychain descriptors.
pub(crate) fn read_keychain_descriptors(
    conn: &Connection,
) -> Result<BTreeMap<KeychainKind, Descriptor<DescriptorPublicKey>>, Error> {
    let mut descriptors = BTreeMap::new();

    let mut stmt = conn.prepare("SELECT keychain, descriptor FROM keychain")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let keychain: u8 = row.get("keychain")?;
        let keychain = match keychain {
            0 => KeychainKind::External,
            1 => KeychainKind::Internal,
            _ => {
                debug_assert!(false, "keychain must map to a value of 0 or 1");
                continue;
            }
        };
        let descriptor: String = row.get("descriptor")?;
        let descriptor = Descriptor::from_str(&descriptor)?;
        descriptors.insert(keychain, descriptor);
    }

    Ok(descriptors)
}

/// Write locked outpoints.
pub(crate) fn write_locked_outpoints(
    conn: &Connection,
    locked_outpoints: &locked_outpoints::ChangeSet,
) -> Result<(), Error> {
    for (&outpoint, &is_locked) in &locked_outpoints.outpoints {
        let OutPoint { txid, vout } = outpoint;
        if is_locked {
            conn.execute(
                "INSERT OR IGNORE INTO locked_outpoint(txid, vout) VALUES(?1, ?2)",
                params![txid.to_string(), vout],
            )?;
        } else {
            conn.execute(
                "DELETE FROM locked_outpoint WHERE txid = ?1 AND vout = ?2",
                params![txid.to_string(), vout],
            )?;
        }
    }

    Ok(())
}

/// Read locked outpoints.
pub(crate) fn read_locked_outpoints(
    conn: &Connection,
) -> Result<locked_outpoints::ChangeSet, Error> {
    let mut changeset = locked_outpoints::ChangeSet::default();

    let mut stmt = conn.prepare("SELECT txid, vout FROM locked_outpoint")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let txid: String = row.get("txid")?;
        let txid: bitcoin::Txid = txid.parse()?;
        let vout: u32 = row.get("vout")?;
        let outpoint = OutPoint { txid, vout };
        changeset.outpoints.insert(outpoint, true);
    }

    Ok(changeset)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn network_table_has_at_most_one_row() -> anyhow::Result<()> {
        let conn = Connection::open_in_memory()?;
        crate::migrate::migrate(&conn)?;

        {
            let txn = conn.unchecked_transaction()?;
            write_network(&txn, Network::Bitcoin)?;
            write_network(&txn, Network::Bitcoin)?;
            txn.commit()?;
        }

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM network", [], |row| row.get(0))?;
        assert_eq!(count, 1, "network table should have at most 1 row");

        Ok(())
    }
}

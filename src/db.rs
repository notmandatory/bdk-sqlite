//! Core persistence routines shared by the blocking and async stores.

use std::sync::Arc;

use bdk_chain::{
    BlockId, ConfirmationBlockTime, DescriptorId, bitcoin, keychain_txout, local_chain, tx_graph,
};
use bitcoin::{Amount, BlockHash, OutPoint, ScriptBuf, Transaction, TxOut, Txid, consensus};
use sqlite_thin::{Connection, params};

use crate::error::InternalError as Error;

/// Opens a database connection from a path or a sqlx-style URL.
pub(crate) fn open(path: &str) -> Result<Connection, Error> {
    let path = path.strip_prefix("sqlite://").unwrap_or(path);
    let conn = match path {
        ":memory:" | "sqlite::memory:" => Connection::open_in_memory()?,
        path => Connection::open(path)?,
    };
    Ok(conn)
}

/// Runs `f` inside a transaction.
pub(crate) fn with_transaction<T>(
    conn: &Connection,
    f: impl FnOnce(&Connection) -> Result<T, Error>,
) -> Result<T, Error> {
    let txn = conn.unchecked_transaction()?;
    let value = f(&txn)?;
    txn.commit()?;
    Ok(value)
}

/// Write tx_graph.
pub(crate) fn write_tx_graph(
    conn: &Connection,
    tx_graph: &tx_graph::ChangeSet<ConfirmationBlockTime>,
) -> Result<(), Error> {
    for tx in &tx_graph.txs {
        let txid = tx.compute_txid();
        conn.execute(
            "INSERT INTO tx(txid, tx) VALUES(?1, ?2) ON CONFLICT DO UPDATE SET tx = ?2",
            params![txid.to_string(), consensus::encode::serialize(tx)],
        )?;
    }
    for (txid, t) in &tx_graph.first_seen {
        conn.execute(
            "INSERT INTO tx(txid, first_seen) VALUES(?1, ?2) ON CONFLICT DO UPDATE SET first_seen = ?2",
            params![txid.to_string(), i64::try_from(*t)?],
        )?;
    }
    for (txid, t) in &tx_graph.last_seen {
        conn.execute(
            "INSERT INTO tx(txid, last_seen) VALUES(?1, ?2) ON CONFLICT DO UPDATE SET last_seen = ?2",
            params![txid.to_string(), i64::try_from(*t)?],
        )?;
    }
    for (txid, t) in &tx_graph.last_evicted {
        conn.execute(
            "INSERT INTO tx(txid, last_evicted) VALUES(?1, ?2) ON CONFLICT DO UPDATE SET last_evicted = ?2",
            params![txid.to_string(), i64::try_from(*t)?],
        )?;
    }
    for (op, txout) in &tx_graph.txouts {
        let OutPoint { txid, vout } = op;
        let TxOut {
            value,
            script_pubkey,
        } = txout;
        conn.execute(
            "INSERT INTO txout(txid, vout, value, script) VALUES(?1, ?2, ?3, ?4) \
             ON CONFLICT DO UPDATE SET value = ?3, script = ?4",
            params![
                txid.to_string(),
                vout,
                i64::try_from(value.to_sat())?,
                script_pubkey.to_bytes()
            ],
        )?;
    }
    for (anchor, txid) in &tx_graph.anchors {
        let BlockId { height, hash } = anchor.block_id;
        let confirmation_time = anchor.confirmation_time;
        conn.execute(
            "INSERT OR IGNORE INTO anchor(block_height, block_hash, txid, confirmation_time) \
             VALUES(?1, ?2, ?3, ?4)",
            params![
                height,
                hash.to_string(),
                txid.to_string(),
                i64::try_from(confirmation_time)?
            ],
        )?;
    }

    Ok(())
}

/// Write local_chain.
pub(crate) fn write_local_chain(
    conn: &Connection,
    local_chain: &local_chain::ChangeSet,
) -> Result<(), Error> {
    for (&height, hash) in &local_chain.blocks {
        match hash {
            Some(hash) => {
                conn.execute(
                    "INSERT OR IGNORE INTO block(height, hash) VALUES(?1, ?2)",
                    params![height, hash.to_string()],
                )?;
            }
            None => {
                conn.execute("DELETE FROM block WHERE height = ?1", params![height])?;
            }
        }
    }

    Ok(())
}

/// Write keychain_txout.
pub(crate) fn write_keychain_txout(
    conn: &Connection,
    keychain_txout: &keychain_txout::ChangeSet,
) -> Result<(), Error> {
    for (descriptor_id, last_revealed) in &keychain_txout.last_revealed {
        conn.execute(
            "INSERT INTO keychain_last_revealed(descriptor_id, last_revealed) VALUES(?1, ?2) \
             ON CONFLICT DO UPDATE SET last_revealed = ?2",
            params![descriptor_id.to_string(), last_revealed],
        )?;
    }
    for (descriptor_id, spk_cache) in &keychain_txout.spk_cache {
        for (derivation_index, script) in spk_cache {
            conn.execute(
                "INSERT OR IGNORE INTO keychain_script_pubkey(descriptor_id, derivation_index, script) \
                 VALUES(?1, ?2, ?3)",
                params![descriptor_id.to_string(), derivation_index, script.to_bytes()],
            )?;
        }
    }

    Ok(())
}

/// Read tx_graph.
pub(crate) fn read_tx_graph(
    conn: &Connection,
) -> Result<tx_graph::ChangeSet<ConfirmationBlockTime>, Error> {
    let mut changeset = tx_graph::ChangeSet::default();

    {
        let mut stmt =
            conn.prepare("SELECT txid, tx, first_seen, last_seen, last_evicted FROM tx")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let txid: String = row.get("txid")?;
            let txid: Txid = txid.parse()?;
            let data: Option<Vec<u8>> = row.get("tx")?;
            if let Some(data) = data {
                let tx: Transaction = consensus::encode::deserialize(&data)?;
                changeset.txs.insert(Arc::new(tx));
            }
            let first_seen: Option<i64> = row.get("first_seen")?;
            if let Some(first_seen) = first_seen {
                changeset.first_seen.insert(txid, first_seen.try_into()?);
            }
            let last_seen: Option<i64> = row.get("last_seen")?;
            if let Some(last_seen) = last_seen {
                changeset.last_seen.insert(txid, last_seen.try_into()?);
            }
            let last_evicted: Option<i64> = row.get("last_evicted")?;
            if let Some(last_evicted) = last_evicted {
                changeset
                    .last_evicted
                    .insert(txid, last_evicted.try_into()?);
            }
        }
    }

    {
        let mut stmt = conn.prepare("SELECT txid, vout, value, script FROM txout")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let txid: String = row.get("txid")?;
            let txid: Txid = txid.parse()?;
            let vout: u32 = row.get("vout")?;
            let value: i64 = row.get("value")?;
            let value = Amount::from_sat(value.try_into()?);
            let script: Vec<u8> = row.get("script")?;
            let script_pubkey = ScriptBuf::from_bytes(script);
            let outpoint = OutPoint { txid, vout };
            let txout = TxOut {
                value,
                script_pubkey,
            };
            changeset.txouts.insert(outpoint, txout);
        }
    }

    {
        let mut stmt =
            conn.prepare("SELECT block_height, block_hash, txid, confirmation_time FROM anchor")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let height: u32 = row.get("block_height")?;
            let hash: String = row.get("block_hash")?;
            let hash: BlockHash = hash.parse()?;
            let txid: String = row.get("txid")?;
            let txid: Txid = txid.parse()?;
            let confirmation_time: i64 = row.get("confirmation_time")?;
            let anchor = ConfirmationBlockTime {
                block_id: BlockId { height, hash },
                confirmation_time: confirmation_time.try_into()?,
            };
            changeset.anchors.insert((anchor, txid));
        }
    }

    Ok(changeset)
}

/// Read local_chain.
pub(crate) fn read_local_chain(conn: &Connection) -> Result<local_chain::ChangeSet, Error> {
    let mut changeset = local_chain::ChangeSet::default();

    let mut stmt = conn.prepare("SELECT height, hash FROM block")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let height: u32 = row.get("height")?;
        let hash: String = row.get("hash")?;
        let hash: BlockHash = hash.parse()?;
        changeset.blocks.insert(height, Some(hash));
    }

    Ok(changeset)
}

/// Read keychain_txout.
pub(crate) fn read_keychain_txout(conn: &Connection) -> Result<keychain_txout::ChangeSet, Error> {
    let mut changeset = keychain_txout::ChangeSet::default();

    {
        let mut stmt =
            conn.prepare("SELECT descriptor_id, last_revealed FROM keychain_last_revealed")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let descriptor_id: String = row.get("descriptor_id")?;
            let descriptor_id: DescriptorId = descriptor_id.parse()?;
            let last_revealed: u32 = row.get("last_revealed")?;
            changeset.last_revealed.insert(descriptor_id, last_revealed);
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT descriptor_id, derivation_index, script FROM keychain_script_pubkey",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let descriptor_id: String = row.get("descriptor_id")?;
            let descriptor_id: DescriptorId = descriptor_id.parse()?;
            let derivation_index: u32 = row.get("derivation_index")?;
            let script: Vec<u8> = row.get("script")?;
            let script = ScriptBuf::from_bytes(script);
            changeset
                .spk_cache
                .entry(descriptor_id)
                .or_default()
                .insert(derivation_index, script);
        }
    }

    Ok(changeset)
}

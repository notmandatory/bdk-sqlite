# `bdk_sqlite`

This crate features the `Store` type, a SQLite store for persisting BDK change
sets. It is built on [`sqlite-thin`], a small safe wrapper around SQLite.

## Example

```rust,no_run
use bdk_sqlite::Store;
use bdk_wallet::bitcoin;
use bdk_wallet::{KeychainKind, Wallet};
use bitcoin::Network;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create `Store`.
    let mut db = Store::new("test.db")?;

    let descriptor = "wpkh([e273fe42/84'/1'/0']tpubDCmr3Luq75npLaYmRqqW1rLfSbfpnBXwLwAmUbR333fp95wjCHar3zoc9zSWovZFwrWr53mm3NTVqt6d1Pt6G26uf4etQjc3Pr5Hxe9QEQ2/0/*)";
    let change_descriptor = "wpkh([e273fe42/84'/1'/0']tpubDCmr3Luq75npLaYmRqqW1rLfSbfpnBXwLwAmUbR333fp95wjCHar3zoc9zSWovZFwrWr53mm3NTVqt6d1Pt6G26uf4etQjc3Pr5Hxe9QEQ2/1/*)";

    // Create `Wallet`.
    let mut wallet = Wallet::create(descriptor, change_descriptor)
        .network(Network::Signet)
        .create_wallet(&mut db)?;

    println!(
        "Address: {}",
        wallet.reveal_next_address(KeychainKind::External),
    );

    // Persist wallet state to SQLite database.
    wallet.persist(&mut db)?;

    Ok(())
}
```

## Async

With the `async` feature enabled, `AsyncStore` implements
[`AsyncWalletPersister`]. It owns a dedicated worker thread and does not depend
on a particular async runtime.

```rust,ignore
use bdk_sqlite::AsyncStore;
use bdk_wallet::bitcoin;
use bdk_wallet::{KeychainKind, Wallet};
use bitcoin::Network;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = AsyncStore::new("test.db").await?;

    let mut wallet = Wallet::create(descriptor, change_descriptor)
        .network(Network::Signet)
        .create_wallet_async(&mut db)
        .await?;

    println!(
        "Address: {}",
        wallet.reveal_next_address(KeychainKind::External),
    );

    wallet.persist_async(&mut db).await?;

    Ok(())
}
```

## Features

* `blocking` - Provides the blocking `Store`, which implements [`WalletPersister`]. Enabled by default.
* `async` - Provides `AsyncStore`, which implements [`AsyncWalletPersister`] using a dedicated worker thread.
* `wallet` - Provides change set support for `bdk_wallet`. Enabled by default.
* `bundled` - Compiles SQLite from source instead of linking the system library. Enabled by default.

The `blocking` and `async` features are additive and may be enabled together.

## Migrations

Schema migrations are embedded in the crate and applied automatically. Applied
versions are recorded in a `schema_migrations` table. Databases created by
earlier sqlx-based releases are imported once, read-only, from the legacy
`_sqlx_migrations` table so existing wallets are never re-migrated.

## MSRV

The Minimum Supported Rust Version (MSRV) is 1.85.0.

[`sqlite-thin`]: https://github.com/thesimplekid/cdk/tree/main/crates/sqlite-thin
[`WalletPersister`]: https://docs.rs/bdk_wallet/latest/bdk_wallet/trait.WalletPersister.html
[`AsyncWalletPersister`]: https://docs.rs/bdk_wallet/latest/bdk_wallet/trait.AsyncWalletPersister.html

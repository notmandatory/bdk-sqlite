use bdk_sqlite::Store;
use bdk_wallet::bitcoin;
use bdk_wallet::{KeychainKind, Wallet};

const DB_PATH: &str = "test_blocking.db";
const NETWORK: bitcoin::Network = bitcoin::Network::Testnet4;
const EXTERNAL_DESC: &str = "wpkh([e273fe42/84'/1'/0']tpubDCmr3Luq75npLaYmRqqW1rLfSbfpnBXwLwAmUbR333fp95wjCHar3zoc9zSWovZFwrWr53mm3NTVqt6d1Pt6G26uf4etQjc3Pr5Hxe9QEQ2/0/*)";
const INTERNAL_DESC: &str = "wpkh([e273fe42/84'/1'/0']tpubDCmr3Luq75npLaYmRqqW1rLfSbfpnBXwLwAmUbR333fp95wjCHar3zoc9zSWovZFwrWr53mm3NTVqt6d1Pt6G26uf4etQjc3Pr5Hxe9QEQ2/1/*)";

fn main() -> anyhow::Result<()> {
    // Create an in memory `Store` for testing.
    // let mut db = Store::new_memory()?;

    // Or connect to a database on disk, creating it if it doesn't exist.
    let mut db = Store::new(DB_PATH)?;

    let mut wallet = match Wallet::load().load_wallet(&mut db)? {
        Some(wallet) => wallet,
        None => Wallet::create(EXTERNAL_DESC, INTERNAL_DESC)
            .network(NETWORK)
            .create_wallet(&mut db)?,
    };

    println!(
        "Address: {}",
        wallet.reveal_next_address(KeychainKind::External)
    );

    // Persist wallet state to the SQLite database.
    wallet.persist(&mut db)?;

    println!("{}", wallet.balance().total());

    Ok(())
}

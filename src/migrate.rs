//! Embedded schema migrations and the migration runner.
//!
//! Applied migrations are recorded in `schema_migrations`. Databases created by
//! older releases of this crate recorded them in sqlx's `_sqlx_migrations`
//! table; that table is imported once (and only read) so existing wallets are
//! never re-migrated from scratch.

use sha2::{Digest, Sha384};
use sqlite_thin::{Connection, OptionalExtension, params};

use crate::error::{Error, MigrateError};

/// Migrations in version order: `(version, description, sql)`.
const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "schema",
        include_str!("../migrations/0001_schema.up.sql"),
    ),
    (
        2,
        "schema",
        include_str!("../migrations/0002_schema.up.sql"),
    ),
    (
        3,
        "schema",
        include_str!("../migrations/0003_schema.up.sql"),
    ),
    (
        4,
        "schema",
        include_str!("../migrations/0004_schema.up.sql"),
    ),
];

const CREATE_TABLE: &str = "\
CREATE TABLE IF NOT EXISTS schema_migrations (
    version     INTEGER PRIMARY KEY,
    description TEXT    NOT NULL,
    applied_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    checksum    BLOB    NOT NULL
);";

/// Name of the table used by previous sqlx-based releases.
const LEGACY_TABLE: &str = "_sqlx_migrations";

/// Runs all pending migrations.
pub(crate) fn migrate(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(CREATE_TABLE)?;
    import_legacy(conn)?;

    for &(version, description, sql) in MIGRATIONS {
        let checksum = checksum(sql);
        let stored: Option<Vec<u8>> = conn
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![version],
                |row| row.get(0),
            )
            .optional()?;

        match stored {
            Some(stored) if stored == checksum => {}
            Some(_) => return Err(MigrateError::VersionMismatch(version).into()),
            None => apply(conn, version, description, sql, &checksum)?,
        }
    }

    Ok(())
}

/// Imports the migration ledger written by sqlx-based releases, once.
fn import_legacy(conn: &Connection) -> Result<(), Error> {
    if !table_exists(conn, LEGACY_TABLE)? {
        return Ok(());
    }

    let applied: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
        row.get(0)
    })?;
    if applied > 0 {
        return Ok(());
    }

    let dirty: Option<i64> = conn
        .query_row(
            "SELECT version FROM _sqlx_migrations WHERE success = 0 ORDER BY version LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(version) = dirty {
        return Err(MigrateError::Dirty(version).into());
    }

    let legacy = {
        let mut stmt = conn.prepare(
            "SELECT version, checksum FROM _sqlx_migrations WHERE success = 1 ORDER BY version",
        )?;
        let mut rows = stmt.query([])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            entries.push((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?));
        }
        entries
    };

    for (version, stored) in legacy {
        let Some(&(_, description, sql)) = MIGRATIONS.iter().find(|(v, _, _)| *v == version) else {
            return Err(MigrateError::VersionMissing(version).into());
        };
        let checksum = checksum(sql);
        if stored != checksum {
            return Err(MigrateError::VersionMismatch(version).into());
        }
        insert(conn, version, description, &checksum)?;
    }

    Ok(())
}

fn apply(
    conn: &Connection,
    version: i64,
    description: &str,
    sql: &str,
    checksum: &[u8],
) -> Result<(), Error> {
    let txn = conn.unchecked_transaction()?;
    txn.execute_batch(sql)?;
    insert(&txn, version, description, checksum)?;
    txn.commit()?;
    Ok(())
}

fn insert(
    conn: &Connection,
    version: i64,
    description: &str,
    checksum: &[u8],
) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO schema_migrations(version, description, checksum) VALUES(?1, ?2, ?3)",
        params![version, description, checksum],
    )?;
    Ok(())
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, Error> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![name],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

fn checksum(sql: &str) -> Vec<u8> {
    Sha384::digest(sql.as_bytes()).to_vec()
}

#[cfg(test)]
mod test {
    use super::*;

    const LEGACY_DDL: &str = "\
CREATE TABLE _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
);";

    fn seed_legacy(conn: &Connection, success: bool) -> Result<(), Error> {
        conn.execute_batch(LEGACY_DDL)?;
        for &(version, description, sql) in MIGRATIONS {
            conn.execute(
                "INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) \
                 VALUES(?1, ?2, ?3, ?4, 0)",
                params![version, description, success, checksum(sql)],
            )?;
        }
        Ok(())
    }

    #[test]
    fn fresh_database_applies_every_migration() -> anyhow::Result<()> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;

        let applied: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
        assert_eq!(applied, MIGRATIONS.len() as i64);

        // Running again is a no-op.
        migrate(&conn)?;
        let applied: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
        assert_eq!(applied, MIGRATIONS.len() as i64);

        // The legacy table is not created for fresh databases.
        assert!(!table_exists(&conn, LEGACY_TABLE)?);

        Ok(())
    }

    #[test]
    fn legacy_ledger_is_imported_without_rerunning_migrations() -> anyhow::Result<()> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;

        // Pretend this database was created by an sqlx-based release.
        conn.execute_batch("DROP TABLE schema_migrations")?;
        seed_legacy(&conn, true)?;

        // Keep a row that migration 0001 would delete if it ran again.
        conn.execute_batch("CREATE TABLE sentinel(value TEXT NOT NULL)")?;
        conn.execute_batch("INSERT INTO sentinel(value) VALUES('keep me')")?;

        migrate(&conn)?;

        let value: String = conn.query_row("SELECT value FROM sentinel", [], |row| row.get(0))?;
        assert_eq!(value, "keep me");

        let applied: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
        assert_eq!(applied, MIGRATIONS.len() as i64);

        Ok(())
    }

    #[test]
    fn legacy_checksum_mismatch_aborts() -> anyhow::Result<()> {
        let conn = Connection::open_in_memory()?;
        seed_legacy(&conn, true)?;
        conn.execute_batch("UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 1")?;

        let err = migrate(&conn).expect_err("checksum mismatch should abort");
        assert!(matches!(
            err,
            Error::Migrate(MigrateError::VersionMismatch(1))
        ));

        // Nothing was imported.
        let applied: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
        assert_eq!(applied, 0);

        Ok(())
    }

    #[test]
    fn legacy_dirty_migration_aborts() -> anyhow::Result<()> {
        let conn = Connection::open_in_memory()?;
        seed_legacy(&conn, false)?;

        let err = migrate(&conn).expect_err("dirty ledger should abort");
        assert!(matches!(err, Error::Migrate(MigrateError::Dirty(1))));

        Ok(())
    }

    #[test]
    fn legacy_unknown_version_aborts() -> anyhow::Result<()> {
        let conn = Connection::open_in_memory()?;
        seed_legacy(&conn, true)?;
        conn.execute(
            "INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) \
             VALUES(?1, 'future', 1, X'00', 0)",
            params![MIGRATIONS.len() as i64 + 1],
        )?;

        let err = migrate(&conn).expect_err("unknown version should abort");
        assert!(matches!(
            err,
            Error::Migrate(MigrateError::VersionMissing(v)) if v == MIGRATIONS.len() as i64 + 1
        ));

        Ok(())
    }
}

//! Phase 9B: copy-only import from legacy CC Switch data directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{backup::Backup, Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::config::get_home_dir;
use crate::error::AppError;

const LEGACY_DIR: &str = ".cc-switch";
const LEGACY_DB: &str = "cc-switch.db";
const AUTOTIER_DB: &str = "autotier.db";
const PENDING_IMPORT_DB: &str = "autotier.db.import-pending";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyDataStatus {
    pub legacy_dir: String,
    pub legacy_db_exists: bool,
    pub autotier_dir: String,
    pub autotier_db_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportLegacyResult {
    pub imported_from: String,
    pub imported_to: String,
    pub backup_path: Option<String>,
}

pub fn legacy_cc_switch_dir() -> PathBuf {
    get_home_dir().join(LEGACY_DIR)
}

pub fn autotier_data_dir() -> PathBuf {
    get_home_dir().join(".autotier")
}

pub fn detect_legacy_data() -> LegacyDataStatus {
    let legacy_dir = legacy_cc_switch_dir();
    let autotier_dir = autotier_data_dir();
    LegacyDataStatus {
        legacy_dir: legacy_dir.display().to_string(),
        legacy_db_exists: legacy_dir.join(LEGACY_DB).is_file(),
        autotier_dir: autotier_dir.display().to_string(),
        autotier_db_exists: autotier_dir.join(AUTOTIER_DB).is_file(),
    }
}

fn verify_sqlite(path: &Path) -> Result<(), AppError> {
    let conn = Connection::open(path)
        .map_err(|e| AppError::Database(format!("import integrity open failed: {e}")))?;
    let result = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .map_err(|e| AppError::Database(format!("import integrity check failed: {e}")))?;
    if result != "ok" {
        return Err(AppError::Database(format!(
            "import integrity check returned: {result}"
        )));
    }
    Ok(())
}

fn backup_sqlite(source: &Path, destination: &Path) -> Result<(), AppError> {
    let source_conn = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| AppError::Database(format!("open backup source failed: {e}")))?;
    let mut destination_conn = Connection::open(destination)
        .map_err(|e| AppError::Database(format!("open backup destination failed: {e}")))?;
    let backup = Backup::new(&source_conn, &mut destination_conn)
        .map_err(|e| AppError::Database(format!("start database backup failed: {e}")))?;
    backup
        .run_to_completion(64, Duration::from_millis(5), None)
        .map_err(|e| AppError::Database(format!("database backup failed: {e}")))?;
    drop(backup);
    drop(destination_conn);
    verify_sqlite(destination)
}

fn remove_sqlite_sidecars(database: &Path) {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", database.display()));
        if let Err(e) = fs::remove_file(&sidecar) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "failed to remove stale SQLite sidecar {}: {e}",
                    sidecar.display()
                );
            }
        }
    }
}

#[tauri::command]
pub fn autotier_detect_legacy_data() -> LegacyDataStatus {
    detect_legacy_data()
}

#[tauri::command]
pub fn autotier_import_legacy_data() -> Result<ImportLegacyResult, String> {
    import_legacy_copy_only().map_err(|e| e.to_string())
}

pub fn import_legacy_copy_only() -> Result<ImportLegacyResult, AppError> {
    let legacy_dir = legacy_cc_switch_dir();
    let legacy_db = legacy_dir.join(LEGACY_DB);
    if !legacy_db.is_file() {
        return Err(AppError::InvalidInput(
            "legacy CC Switch database not found".into(),
        ));
    }

    let target_dir = autotier_data_dir();
    fs::create_dir_all(&target_dir).map_err(|e| AppError::io(&target_dir, e))?;
    let target_db = target_dir.join(AUTOTIER_DB);
    let staging = target_dir.join(format!(".import-staging-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&staging).map_err(|e| AppError::io(&staging, e))?;
    backup_sqlite(&legacy_db, &staging.join("db.tmp"))?;
    verify_sqlite(&staging.join("db.tmp"))?;

    let backup_path = if target_db.is_file() {
        let backup_dir = target_dir.join("backups");
        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
        let backup_path = backup_dir.join(format!(
            "autotier-pre-legacy-import-{}.db",
            uuid::Uuid::new_v4()
        ));
        backup_sqlite(&target_db, &backup_path)?;
        Some(backup_path)
    } else {
        None
    };

    let pending_db = target_dir.join(PENDING_IMPORT_DB);
    if pending_db.exists() {
        fs::remove_file(&pending_db).map_err(|e| AppError::io(&pending_db, e))?;
    }
    fs::rename(staging.join("db.tmp"), &pending_db).map_err(|e| AppError::io(&pending_db, e))?;
    let _ = fs::remove_dir(&staging);

    Ok(ImportLegacyResult {
        imported_from: legacy_db.display().to_string(),
        imported_to: target_db.display().to_string(),
        backup_path: backup_path.map(|path| path.display().to_string()),
    })
}

/// Applies a previously staged legacy import before the application opens its
/// main database connection. Failure leaves the current database untouched and
/// keeps the pending file for a later retry.
pub fn apply_pending_legacy_import() -> Result<bool, AppError> {
    let target_dir = autotier_data_dir();
    let pending_db = target_dir.join(PENDING_IMPORT_DB);
    if !pending_db.is_file() {
        return Ok(false);
    }
    verify_sqlite(&pending_db)?;

    let target_db = target_dir.join(AUTOTIER_DB);
    let recovery_backup = if target_db.is_file() {
        let backup_dir = target_dir.join("backups");
        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
        let path = backup_dir.join(format!(
            "autotier-at-legacy-import-{}.db",
            uuid::Uuid::new_v4()
        ));
        backup_sqlite(&target_db, &path)?;
        Some(path)
    } else {
        None
    };

    if target_db.exists() {
        fs::remove_file(&target_db).map_err(|e| AppError::io(&target_db, e))?;
    }
    remove_sqlite_sidecars(&target_db);
    if let Err(e) = fs::rename(&pending_db, &target_db) {
        if let Some(backup) = recovery_backup.as_ref() {
            let _ = fs::copy(backup, &target_db);
        }
        return Err(AppError::io(&target_db, e));
    }
    verify_sqlite(&target_db)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn detect_legacy_reports_paths() {
        let home = std::env::temp_dir().join(format!("autotier-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join(LEGACY_DIR)).unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", &home);
        std::env::set_var("HOME", &home);
        let status = detect_legacy_data();
        assert!(status.legacy_dir.contains(".cc-switch"));
        assert!(status.autotier_dir.contains(".autotier"));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    #[serial_test::serial]
    fn import_copy_only_leaves_legacy_intact() {
        let home = std::env::temp_dir().join(format!("autotier-import-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join(LEGACY_DIR)).unwrap();
        let legacy_db = home.join(LEGACY_DIR).join(LEGACY_DB);
        let conn = Connection::open(&legacy_db).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE autotier_routing_config (
               id INTEGER PRIMARY KEY,
               mode TEXT NOT NULL,
               retention_days INTEGER NOT NULL,
               raw_prompt_opt_in INTEGER NOT NULL,
               classifier_version TEXT NOT NULL,
               feature_version TEXT NOT NULL,
               policy_version TEXT NOT NULL,
               updated_at INTEGER NOT NULL
             );",
        )
        .unwrap();

        std::env::set_var("AUTOTIER_TEST_HOME", &home);
        std::env::set_var("HOME", &home);
        let before = std::fs::metadata(&legacy_db).unwrap().len();
        let result = import_legacy_copy_only().expect("import");
        assert!(!PathBuf::from(&result.imported_to).exists());
        assert!(apply_pending_legacy_import().expect("apply pending import"));
        assert!(PathBuf::from(&result.imported_to).exists());
        assert_eq!(std::fs::metadata(&legacy_db).unwrap().len(), before);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    #[serial_test::serial]
    fn existing_autotier_database_is_backed_up_before_restart_import() {
        let home = std::env::temp_dir().join(format!("autotier-reimport-{}", uuid::Uuid::new_v4()));
        let legacy_dir = home.join(LEGACY_DIR);
        let target_dir = home.join(".autotier");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();

        let legacy_db = legacy_dir.join(LEGACY_DB);
        Connection::open(&legacy_db)
            .unwrap()
            .execute("CREATE TABLE legacy_marker(value TEXT)", [])
            .unwrap();
        let target_db = target_dir.join(AUTOTIER_DB);
        Connection::open(&target_db)
            .unwrap()
            .execute("CREATE TABLE current_marker(value TEXT)", [])
            .unwrap();

        std::env::set_var("AUTOTIER_TEST_HOME", &home);
        std::env::set_var("HOME", &home);
        let result = import_legacy_copy_only().expect("stage import");
        let backup = PathBuf::from(result.backup_path.expect("current database backup"));
        assert!(backup.is_file());
        assert!(apply_pending_legacy_import().expect("apply import"));

        let imported = Connection::open(&target_db).unwrap();
        assert!(imported.prepare("SELECT * FROM legacy_marker").is_ok());
        let saved_current = Connection::open(backup).unwrap();
        assert!(saved_current
            .prepare("SELECT * FROM current_marker")
            .is_ok());
        let _ = std::fs::remove_dir_all(home);
    }
}

//! Phase 9B: copy-only import from legacy CC Switch data directory.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::config::get_home_dir;
use crate::error::AppError;

const LEGACY_DIR: &str = ".cc-switch";
const LEGACY_DB: &str = "cc-switch.db";
const AUTOTIER_DB: &str = "autotier.db";

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
        autotier_db_exists: autotier_dir.join(AUTOTIER_DB).is_file()
            || autotier_dir.join(LEGACY_DB).is_file(),
    }
}

fn verify_sqlite(path: &Path) -> Result<(), AppError> {
    let conn = Connection::open(path)
        .map_err(|e| AppError::Database(format!("import integrity open failed: {e}")))?;
    conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .map_err(|e| AppError::Database(format!("import integrity check failed: {e}")))?;
    Ok(())
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
    if target_db.exists() {
        return Err(AppError::InvalidInput(
            "AutoTier database already exists; import aborted to avoid overwrite".into(),
        ));
    }

    let staging = target_dir.join(format!(".import-staging-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&staging).map_err(|e| AppError::io(&staging, e))?;
    fs::copy(&legacy_db, staging.join("db.tmp")).map_err(|e| AppError::io(&legacy_db, e))?;
    verify_sqlite(&staging.join("db.tmp"))?;
    fs::rename(staging.join("db.tmp"), &target_db).map_err(|e| AppError::io(&target_db, e))?;
    let _ = fs::remove_dir(&staging);

    Ok(ImportLegacyResult {
        imported_from: legacy_db.display().to_string(),
        imported_to: target_db.display().to_string(),
        backup_path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_legacy_reports_paths() {
        let home = std::env::temp_dir().join(format!("autotier-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join(LEGACY_DIR)).unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", &home);
        std::env::set_var("HOME", &home);
        let status = detect_legacy_data();
        assert!(status.legacy_dir.contains(".cc-switch"));
        assert!(status.autotier_dir.contains(".autotier"));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
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

        std::env::set_var("CC_SWITCH_TEST_HOME", &home);
        std::env::set_var("HOME", &home);
        let before = std::fs::metadata(&legacy_db).unwrap().len();
        let result = import_legacy_copy_only().expect("import");
        assert!(PathBuf::from(&result.imported_to).exists());
        assert_eq!(std::fs::metadata(&legacy_db).unwrap().len(), before);
        let _ = std::fs::remove_dir_all(home);
    }
}

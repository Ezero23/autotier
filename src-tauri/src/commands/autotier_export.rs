//! Phase 7C: privacy-safe export command.

use tauri::State;

use crate::autotier::export::{export_bundle, validate_export_dir, ExportBundleResult};
use crate::error::AppError;
use crate::store::AppState;

#[tauri::command]
pub fn autotier_export_decisions(
    state: State<'_, AppState>,
    output_dir: String,
) -> Result<ExportBundleResult, String> {
    export_decisions(&state.db, &output_dir).map_err(|e| e.to_string())
}

pub(crate) fn export_decisions(
    db: &crate::database::Database,
    output_dir: &str,
) -> Result<ExportBundleResult, AppError> {
    let path = validate_export_dir(output_dir)?;
    export_bundle(db, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_export_path() {
        let db = crate::database::Database::memory().unwrap();
        let err = export_decisions(&db, "relative/path").unwrap_err();
        assert!(err.to_string().contains("absolute path"));
    }
}

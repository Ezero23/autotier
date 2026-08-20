//! Phase 8A/8B replay and evaluation commands.

use std::path::Path;

use crate::autotier::eval::{evaluate_export_dir, EvalReport, DEFAULT_SPLIT_SEED};
use crate::autotier::export::validate_export_dir;
use crate::autotier::replay::{replay_export_dir, ReplayReport};
use crate::error::AppError;

#[tauri::command]
pub fn autotier_replay_export(export_dir: String) -> Result<ReplayReport, String> {
    replay_export(&export_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_evaluate_export(
    export_dir: String,
    split_seed: Option<u64>,
) -> Result<EvalReport, String> {
    evaluate_export(&export_dir, split_seed).map_err(|e| e.to_string())
}

pub(crate) fn replay_export(export_dir: &str) -> Result<ReplayReport, AppError> {
    let path = validate_export_dir(export_dir)?;
    replay_export_dir(Path::new(&path))
}

pub(crate) fn evaluate_export(
    export_dir: &str,
    split_seed: Option<u64>,
) -> Result<EvalReport, AppError> {
    let path = validate_export_dir(export_dir)?;
    evaluate_export_dir(Path::new(&path), split_seed.unwrap_or(DEFAULT_SPLIT_SEED))
}

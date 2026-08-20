//! Privacy-safe AutoTier export bundle (Phase 7C).
//!
//! Writes `manifest.json`, `decisions.jsonl`, and `labels.jsonl` atomically.
//! Default export contains derived features only — no raw prompts or credentials.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::atomic_write;
use crate::database::{
    AutotierDecisionLabelDto, AutotierDecisionQueryFilter, AutotierDecisionRow, Database,
};
use crate::error::AppError;

pub const EXPORT_SCHEMA_VERSION: i32 = 1;
const MAX_EXPORT_DECISIONS: i64 = 500_000;
const MAX_EXPORT_BYTES: u64 = 256 * 1024 * 1024;
const CANARY_PATTERNS: &[&str] = &[
    "sk-ant-",
    "sk-",
    "Bearer ",
    "api_key",
    "CANARY_PROMPT_SECRET",
    "authorization:",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportTimeRange {
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportManifest {
    pub export_schema_version: i32,
    pub generated_at: String,
    pub time_range: ExportTimeRange,
    pub feature_versions: Vec<String>,
    pub classifier_versions: Vec<String>,
    pub policy_versions: Vec<String>,
    pub hash_algorithm: String,
    pub hash_scope: String,
    pub contains_raw_prompt: bool,
    pub contains_credentials: bool,
    pub decision_count: i64,
    pub label_count: i64,
    pub split_seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportDecisionLine {
    pub decision_id: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub app_type: String,
    pub session_id_hash: String,
    pub mode: String,
    pub client_requested_model: String,
    pub initial_selected_provider: Option<String>,
    pub baseline_outbound_model: Option<String>,
    pub baseline_outbound_provider: Option<String>,
    pub recommended_slot: Option<String>,
    pub candidate_model: Option<String>,
    pub candidate_provider: Option<String>,
    pub actual_outbound_model: Option<String>,
    pub actual_outbound_provider: Option<String>,
    pub complexity_score: Option<f64>,
    pub confidence: Option<f64>,
    pub reason_codes_json: String,
    pub unsafe_reasons_json: String,
    pub safe_to_execute: bool,
    pub feature_json: String,
    pub feature_version: String,
    pub classifier_version: String,
    pub policy_version: String,
    pub actual_cost_usd: Option<String>,
    pub candidate_cost_low_usd: Option<String>,
    pub candidate_cost_base_usd: Option<String>,
    pub candidate_cost_high_usd: Option<String>,
    pub is_complete: bool,
    pub usage_request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportBundleResult {
    pub output_dir: String,
    pub manifest: ExportManifest,
}

pub fn validate_export_dir(path: &str) -> Result<PathBuf, AppError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(
            "export directory is required".into(),
        ));
    }
    if trimmed.len() > 512 {
        return Err(AppError::InvalidInput("export path too long".into()));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(AppError::InvalidInput(
            "export directory must be an absolute path".into(),
        ));
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(AppError::InvalidInput(
                "export path must not contain parent references".into(),
            ));
        }
    }
    Ok(path)
}

fn row_to_export_line(row: &AutotierDecisionRow) -> ExportDecisionLine {
    ExportDecisionLine {
        decision_id: row.decision_id.clone(),
        created_at: row.created_at,
        completed_at: row.completed_at,
        app_type: row.app_type.clone(),
        session_id_hash: row.session_id_hash.clone(),
        mode: row.mode.clone(),
        client_requested_model: row.client_requested_model.clone(),
        initial_selected_provider: row.initial_selected_provider.clone(),
        baseline_outbound_model: row.baseline_outbound_model.clone(),
        baseline_outbound_provider: row.baseline_outbound_provider.clone(),
        recommended_slot: row.recommended_slot.clone(),
        candidate_model: row.candidate_model.clone(),
        candidate_provider: row.candidate_provider.clone(),
        actual_outbound_model: row.actual_outbound_model.clone(),
        actual_outbound_provider: row.actual_outbound_provider.clone(),
        complexity_score: row.complexity_score,
        confidence: row.confidence,
        reason_codes_json: row.reason_codes_json.clone(),
        unsafe_reasons_json: row.unsafe_reasons_json.clone(),
        safe_to_execute: row.safe_to_execute,
        feature_json: row.feature_json.clone(),
        feature_version: row.feature_version.clone(),
        classifier_version: row.classifier_version.clone(),
        policy_version: row.policy_version.clone(),
        actual_cost_usd: row.actual_cost_usd.clone(),
        candidate_cost_low_usd: row.candidate_cost_low_usd.clone(),
        candidate_cost_base_usd: row.candidate_cost_base_usd.clone(),
        candidate_cost_high_usd: row.candidate_cost_high_usd.clone(),
        is_complete: row.is_complete,
        usage_request_id: row.usage_request_id.clone(),
    }
}

fn collect_decisions(db: &Database) -> Result<Vec<AutotierDecisionRow>, AppError> {
    let mut rows = Vec::new();
    let mut offset = 0i64;
    loop {
        let page = db.autotier_query_decisions(&AutotierDecisionQueryFilter {
            limit: Some(100),
            offset: Some(offset),
            ..Default::default()
        })?;
        if page.items.is_empty() {
            break;
        }
        let batch_len = page.items.len();
        for item in page.items {
            if let Some(detail) = db.autotier_get_decision_detail(&item.decision_id)? {
                rows.push(AutotierDecisionRow {
                    decision_id: detail.decision_id,
                    created_at: detail.created_at,
                    completed_at: detail.completed_at,
                    app_type: detail.app_type,
                    session_id_hash: detail.session_id_hash,
                    mode: detail.mode,
                    client_requested_model: detail.client_requested_model,
                    initial_selected_provider: detail.initial_selected_provider,
                    baseline_outbound_model: detail.baseline_outbound_model,
                    baseline_outbound_provider: detail.baseline_outbound_provider,
                    recommended_slot: detail.recommended_slot,
                    candidate_model: detail.candidate_model,
                    candidate_provider: detail.candidate_provider,
                    actual_outbound_model: detail.actual_outbound_model,
                    actual_outbound_provider: detail.actual_outbound_provider,
                    autotier_mutated_request: detail.autotier_mutated_request,
                    upstream_message_id: detail.upstream_message_id,
                    usage_request_id: detail.usage_request_id,
                    complexity_score: Some(detail.complexity_score),
                    confidence: Some(detail.confidence),
                    reason_codes_json: detail.reason_codes_json,
                    unsafe_reasons_json: detail.unsafe_reasons_json,
                    safe_to_execute: detail.safe_to_execute,
                    feature_json: detail.feature_json,
                    feature_version: detail.feature_version,
                    classifier_version: detail.classifier_version,
                    policy_version: detail.policy_version,
                    actual_input_tokens: detail.actual_input_tokens,
                    actual_output_tokens: detail.actual_output_tokens,
                    actual_cache_read_tokens: detail.actual_cache_read_tokens,
                    actual_cache_write_5m_tokens: detail.actual_cache_write_5m_tokens,
                    actual_cache_write_1h_tokens: detail.actual_cache_write_1h_tokens,
                    actual_cost_usd: detail.actual_cost_usd,
                    candidate_cost_low_usd: detail.candidate_cost_low_usd,
                    candidate_cost_base_usd: detail.candidate_cost_base_usd,
                    candidate_cost_high_usd: detail.candidate_cost_high_usd,
                    cost_assumptions_json: detail.cost_assumptions_json,
                    status_code: detail.status_code,
                    outcome: detail.outcome,
                    retry_count: detail.retry_count,
                    fallback_count: detail.fallback_count,
                    is_complete: detail.is_complete,
                    error_code: detail.error_code,
                });
            }
        }
        offset += batch_len as i64;
        if offset >= page.total || offset >= MAX_EXPORT_DECISIONS {
            break;
        }
    }
    Ok(rows)
}

fn collect_labels(
    db: &Database,
    decision_ids: &[String],
) -> Result<Vec<AutotierDecisionLabelDto>, AppError> {
    let mut labels = Vec::new();
    for id in decision_ids {
        if let Some(label) = db.autotier_get_label(id)? {
            labels.push(label);
        }
    }
    Ok(labels)
}

pub fn scan_export_secrets(content: &str) -> Result<(), AppError> {
    let lower = content.to_ascii_lowercase();
    for pattern in CANARY_PATTERNS {
        if lower.contains(&pattern.to_ascii_lowercase()) {
            return Err(AppError::InvalidInput(format!(
                "export failed secret scan: matched pattern `{pattern}`"
            )));
        }
    }
    Ok(())
}

fn scan_path(path: &Path) -> Result<(), AppError> {
    let content = fs::read_to_string(path)
        .map_err(|e| AppError::Database(format!("export scan read failed: {e}")))?;
    scan_export_secrets(&content)
}

pub fn export_bundle(db: &Database, output_dir: &Path) -> Result<ExportBundleResult, AppError> {
    let rows = collect_decisions(db)?;
    if rows.len() as i64 > MAX_EXPORT_DECISIONS {
        return Err(AppError::InvalidInput(format!(
            "export exceeds max decision count ({MAX_EXPORT_DECISIONS})"
        )));
    }

    let decision_ids: Vec<String> = rows.iter().map(|r| r.decision_id.clone()).collect();
    let labels = collect_labels(db, &decision_ids)?;

    let mut feature_versions = BTreeSet::new();
    let mut classifier_versions = BTreeSet::new();
    let mut policy_versions = BTreeSet::new();
    let mut since_ms: Option<i64> = None;
    let mut until_ms: Option<i64> = None;

    for row in &rows {
        feature_versions.insert(row.feature_version.clone());
        classifier_versions.insert(row.classifier_version.clone());
        policy_versions.insert(row.policy_version.clone());
        since_ms = Some(since_ms.map_or(row.created_at, |v| v.min(row.created_at)));
        until_ms = Some(until_ms.map_or(row.created_at, |v| v.max(row.created_at)));
    }

    let manifest = ExportManifest {
        export_schema_version: EXPORT_SCHEMA_VERSION,
        generated_at: Utc::now().to_rfc3339(),
        time_range: ExportTimeRange { since_ms, until_ms },
        feature_versions: feature_versions.into_iter().collect(),
        classifier_versions: classifier_versions.into_iter().collect(),
        policy_versions: policy_versions.into_iter().collect(),
        hash_algorithm: "HMAC-SHA-256".into(),
        hash_scope: "install".into(),
        contains_raw_prompt: false,
        contains_credentials: false,
        decision_count: rows.len() as i64,
        label_count: labels.len() as i64,
        split_seed: None,
    };

    let tmp = output_dir.join(format!(".autotier-export-{}", Uuid::new_v4()));
    fs::create_dir_all(&tmp)
        .map_err(|e| AppError::Database(format!("export temp dir failed: {e}")))?;

    let cleanup = || {
        let _ = fs::remove_dir_all(&tmp);
    };

    let decisions_path = tmp.join("decisions.jsonl");
    let labels_path = tmp.join("labels.jsonl");
    let manifest_path = tmp.join("manifest.json");

    let mut decisions_body = String::new();
    for row in &rows {
        let line = row_to_export_line(row);
        let json = serde_json::to_string(&line)
            .map_err(|e| AppError::Database(format!("export decision json failed: {e}")))?;
        scan_export_secrets(&json)?;
        decisions_body.push_str(&json);
        decisions_body.push('\n');
    }

    let mut labels_body = String::new();
    for label in &labels {
        let json = serde_json::to_string(label)
            .map_err(|e| AppError::Database(format!("export label json failed: {e}")))?;
        scan_export_secrets(&json)?;
        labels_body.push_str(&json);
        labels_body.push('\n');
    }

    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| AppError::Database(format!("export manifest json failed: {e}")))?;
    scan_export_secrets(&manifest_json)?;

    atomic_write(&decisions_path, decisions_body.as_bytes())?;
    atomic_write(&labels_path, labels_body.as_bytes())?;
    atomic_write(&manifest_path, manifest_json.as_bytes())?;

    let total_bytes = fs::metadata(&decisions_path).map(|m| m.len()).unwrap_or(0)
        + fs::metadata(&labels_path).map(|m| m.len()).unwrap_or(0)
        + fs::metadata(&manifest_path).map(|m| m.len()).unwrap_or(0);
    if total_bytes > MAX_EXPORT_BYTES {
        cleanup();
        return Err(AppError::InvalidInput(format!(
            "export exceeds max size ({MAX_EXPORT_BYTES} bytes)"
        )));
    }

    for file in [&decisions_path, &labels_path, &manifest_path] {
        if let Err(err) = scan_path(file) {
            cleanup();
            return Err(err);
        }
    }

    fs::create_dir_all(output_dir)
        .map_err(|e| AppError::Database(format!("export output dir failed: {e}")))?;

    for name in ["decisions.jsonl", "labels.jsonl", "manifest.json"] {
        let from = tmp.join(name);
        let to = output_dir.join(name);
        if to.exists() {
            fs::remove_file(&to)
                .map_err(|e| AppError::Database(format!("export replace failed: {e}")))?;
        }
        fs::rename(&from, &to)
            .map_err(|e| AppError::Database(format!("export finalize failed: {e}")))?;
    }
    let _ = fs::remove_dir(&tmp);

    Ok(ExportBundleResult {
        output_dir: output_dir.display().to_string(),
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn sample_row(id: &str) -> AutotierDecisionRow {
        AutotierDecisionRow {
            decision_id: id.to_string(),
            created_at: 1_700_000_000,
            completed_at: None,
            app_type: "claude".to_string(),
            session_id_hash: "hash-001".to_string(),
            mode: "shadow".to_string(),
            client_requested_model: "claude-sonnet-4-20250514".to_string(),
            initial_selected_provider: Some("provider-a".to_string()),
            baseline_outbound_model: Some("claude-sonnet-4-20250514".to_string()),
            baseline_outbound_provider: Some("provider-a".to_string()),
            recommended_slot: Some("cheap".to_string()),
            candidate_model: Some("claude-haiku".to_string()),
            candidate_provider: Some("provider-a".to_string()),
            actual_outbound_model: Some("claude-sonnet-4-20250514".to_string()),
            actual_outbound_provider: Some("provider-a".to_string()),
            autotier_mutated_request: false,
            upstream_message_id: None,
            usage_request_id: None,
            complexity_score: Some(0.2),
            confidence: Some(0.8),
            reason_codes_json: r#"["SHORT_USER_REQUEST"]"#.to_string(),
            unsafe_reasons_json: "[]".to_string(),
            safe_to_execute: false,
            feature_json: r#"{"extraction_status":"success"}"#.to_string(),
            feature_version: "claude-extractor-v0.2".to_string(),
            classifier_version: "rules-v0.2".to_string(),
            policy_version: "shadow-policy-v0.2".to_string(),
            actual_input_tokens: None,
            actual_output_tokens: None,
            actual_cache_read_tokens: None,
            actual_cache_write_5m_tokens: None,
            actual_cache_write_1h_tokens: None,
            actual_cost_usd: None,
            candidate_cost_low_usd: Some("0.001".into()),
            candidate_cost_base_usd: Some("0.002".into()),
            candidate_cost_high_usd: Some("0.003".into()),
            cost_assumptions_json: "[]".to_string(),
            status_code: None,
            outcome: None,
            retry_count: 0,
            fallback_count: 0,
            is_complete: false,
            error_code: None,
        }
    }

    #[test]
    fn secret_scan_rejects_canary() {
        assert!(scan_export_secrets("hello CANARY_PROMPT_SECRET world").is_err());
    }

    #[test]
    fn export_writes_atomic_bundle() {
        let db = Database::memory().expect("memory db");
        db.autotier_upsert_decision(&sample_row("d-export"))
            .unwrap();
        db.autotier_upsert_label(&AutotierDecisionLabelDto {
            decision_id: "d-export".into(),
            label: "correct".into(),
            reason: None,
            note: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();

        let dir = std::env::temp_dir().join(format!("autotier-export-test-{}", Uuid::new_v4()));
        let result = export_bundle(&db, &dir).expect("export");
        assert_eq!(result.manifest.decision_count, 1);
        assert_eq!(result.manifest.label_count, 1);
        assert!(dir.join("manifest.json").exists());
        assert!(dir.join("decisions.jsonl").exists());
        assert!(dir.join("labels.jsonl").exists());
        let _ = fs::remove_dir_all(dir);
    }
}

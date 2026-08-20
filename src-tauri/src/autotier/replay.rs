//! Deterministic replay of exported Shadow decisions (Phase 8A).
//!
//! Reads a Phase 7C export bundle and re-runs the rule classifier on stored
//! features. Never touches the production database.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app_config::AppType;
use crate::autotier::export::{ExportDecisionLine, ExportManifest, EXPORT_SCHEMA_VERSION};
use crate::autotier::{
    shadow_decide, DecisionId, DecisionInput, RoutingMode, RoutingSessionState,
    CLASSIFIER_VERSION, POLICY_VERSION,
};
use crate::autotier::features::RoutingFeatures;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayMismatch {
    pub decision_id: String,
    pub field: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayMalformedRow {
    pub line_number: usize,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayReport {
    pub replayed: usize,
    pub matched: usize,
    pub mismatches: Vec<ReplayMismatch>,
    pub malformed_rows: Vec<ReplayMalformedRow>,
    pub manifest: ExportManifest,
}

fn parse_app_type(raw: &str) -> AppType {
    match raw.to_ascii_lowercase().as_str() {
        "codex" => AppType::Codex,
        "gemini" => AppType::Gemini,
        "grokbuild" => AppType::GrokBuild,
        "opencode" => AppType::OpenCode,
        "openclaw" => AppType::OpenClaw,
        "hermes" => AppType::Hermes,
        _ => AppType::Claude,
    }
}

fn decision_input_from_line(line: &ExportDecisionLine) -> Result<DecisionInput, AppError> {
    let features: RoutingFeatures = serde_json::from_str(&line.feature_json)
        .map_err(|e| AppError::InvalidInput(format!("feature_json parse failed: {e}")))?;
    Ok(DecisionInput {
        decision_id: DecisionId(line.decision_id.clone()),
        app_type: parse_app_type(&line.app_type),
        client_requested_model: line.client_requested_model.clone(),
        initial_selected_provider: line.initial_selected_provider.clone(),
        features,
        session_state: RoutingSessionState::default(),
        mode: RoutingMode::Shadow,
        feature_version: line.feature_version.clone(),
    })
}

pub fn load_export_manifest(export_dir: &Path) -> Result<ExportManifest, AppError> {
    let path = export_dir.join("manifest.json");
    let raw = fs::read_to_string(&path)
        .map_err(|e| AppError::InvalidInput(format!("manifest read failed: {e}")))?;
    let manifest: ExportManifest = serde_json::from_str(&raw)
        .map_err(|e| AppError::InvalidInput(format!("manifest parse failed: {e}")))?;
    if manifest.export_schema_version != EXPORT_SCHEMA_VERSION {
        return Err(AppError::InvalidInput(format!(
            "unsupported export_schema_version: {}",
            manifest.export_schema_version
        )));
    }
    Ok(manifest)
}

pub fn replay_export_dir(export_dir: &Path) -> Result<ReplayReport, AppError> {
    let manifest = load_export_manifest(export_dir)?;
    if !manifest.classifier_versions.is_empty()
        && !manifest
            .classifier_versions
            .iter()
            .all(|v| v == CLASSIFIER_VERSION)
    {
        return Err(AppError::InvalidInput(format!(
            "classifier version mismatch: export={:?}, runtime={CLASSIFIER_VERSION}",
            manifest.classifier_versions
        )));
    }
    if !manifest.policy_versions.is_empty()
        && !manifest
            .policy_versions
            .iter()
            .all(|v| v == POLICY_VERSION)
    {
        return Err(AppError::InvalidInput(format!(
            "policy version mismatch: export={:?}, runtime={POLICY_VERSION}",
            manifest.policy_versions
        )));
    }

    let decisions_path = export_dir.join("decisions.jsonl");
    let raw = fs::read_to_string(&decisions_path)
        .map_err(|e| AppError::InvalidInput(format!("decisions.jsonl read failed: {e}")))?;

    let mut replayed = 0usize;
    let mut matched = 0usize;
    let mut mismatches = Vec::new();
    let mut malformed_rows = Vec::new();

    for (idx, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: ExportDecisionLine = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(err) => {
                malformed_rows.push(ReplayMalformedRow {
                    line_number: idx + 1,
                    error: err.to_string(),
                });
                continue;
            }
        };

        let input = match decision_input_from_line(&parsed) {
            Ok(value) => value,
            Err(err) => {
                malformed_rows.push(ReplayMalformedRow {
                    line_number: idx + 1,
                    error: err.to_string(),
                });
                continue;
            }
        };

        let result = shadow_decide(&input, parsed.created_at as u64);
        replayed += 1;

        let expected_slot = parsed.recommended_slot.clone().unwrap_or_default();
        let actual_slot = result
            .recommended_slot
            .map(|slot| slot.as_str().to_string())
            .unwrap_or_default();
        let expected_norm = expected_slot.to_ascii_lowercase();

        let slot_match = expected_norm == actual_slot;
        let score_match = parsed
            .complexity_score
            .map(|score| (score - f64::from(result.complexity_score)).abs() < f64::EPSILON)
            .unwrap_or(true);

        if slot_match && score_match {
            matched += 1;
        } else {
            if !slot_match {
                mismatches.push(ReplayMismatch {
                    decision_id: parsed.decision_id.clone(),
                    field: "recommended_slot".into(),
                    expected: expected_slot,
                    actual: actual_slot,
                });
            }
            if !score_match {
                mismatches.push(ReplayMismatch {
                    decision_id: parsed.decision_id.clone(),
                    field: "complexity_score".into(),
                    expected: parsed
                        .complexity_score
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    actual: result.complexity_score.to_string(),
                });
            }
        }
    }

    Ok(ReplayReport {
        replayed,
        matched,
        mismatches,
        malformed_rows,
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autotier::export::{export_bundle, ExportManifest, ExportTimeRange};
    use crate::autotier::{DecisionId, DecisionInput, RoutingMode, RoutingSessionState, shadow_decide};
    use crate::database::{AutotierDecisionRow, Database};

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
            feature_json: serde_json::to_string(&RoutingFeatures::empty(
                AppType::Claude,
                "claude-sonnet-4-20250514",
                "hash-001",
            ))
            .unwrap(),
            feature_version: "claude-extractor-v0.2".to_string(),
            classifier_version: CLASSIFIER_VERSION.to_string(),
            policy_version: POLICY_VERSION.to_string(),
            actual_input_tokens: None,
            actual_output_tokens: None,
            actual_cache_read_tokens: None,
            actual_cache_write_5m_tokens: None,
            actual_cache_write_1h_tokens: None,
            actual_cost_usd: None,
            candidate_cost_low_usd: None,
            candidate_cost_base_usd: None,
            candidate_cost_high_usd: None,
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
    fn replay_is_deterministic_from_export() {
        let features = RoutingFeatures::empty(
            AppType::Claude,
            "claude-sonnet-4-20250514",
            "hash-001",
        );
        let input = DecisionInput {
            decision_id: DecisionId("d-replay".into()),
            app_type: AppType::Claude,
            client_requested_model: "claude-sonnet-4-20250514".to_string(),
            initial_selected_provider: Some("provider-a".to_string()),
            features: features.clone(),
            session_state: RoutingSessionState::default(),
            mode: RoutingMode::Shadow,
            feature_version: "claude-extractor-v0.2".to_string(),
        };
        let decided = shadow_decide(&input, 1_700_000_000);

        let mut row = sample_row("d-replay");
        row.feature_json = serde_json::to_string(&features).unwrap();
        row.recommended_slot = decided
            .recommended_slot
            .map(|slot| slot.as_str().to_string());
        row.complexity_score = Some(f64::from(decided.complexity_score));
        row.confidence = Some(f64::from(decided.confidence));

        let db = Database::memory().expect("memory db");
        db.autotier_upsert_decision(&row).unwrap();
        let dir = std::env::temp_dir().join(format!("autotier-replay-{}", uuid::Uuid::new_v4()));
        export_bundle(&db, &dir).expect("export");
        let report = replay_export_dir(&dir).expect("replay");
        assert_eq!(report.replayed, 1);
        assert_eq!(report.matched, 1);
        assert!(report.mismatches.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn replay_rejects_incompatible_manifest() {
        let dir = std::env::temp_dir().join(format!("autotier-replay-bad-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let manifest = ExportManifest {
            export_schema_version: 999,
            generated_at: "now".into(),
            time_range: ExportTimeRange {
                since_ms: None,
                until_ms: None,
            },
            feature_versions: vec![],
            classifier_versions: vec![],
            policy_versions: vec![],
            hash_algorithm: "HMAC-SHA-256".into(),
            hash_scope: "install".into(),
            contains_raw_prompt: false,
            contains_credentials: false,
            decision_count: 0,
            label_count: 0,
            split_seed: None,
        };
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("decisions.jsonl"), "").unwrap();
        let err = replay_export_dir(&dir).unwrap_err();
        assert!(err.to_string().contains("export_schema_version"));
        let _ = fs::remove_dir_all(dir);
    }
}

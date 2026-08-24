//! Phase 7A：Decision 列表/详情/标注命令层。

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::database::{
    AutotierDecisionDetail, AutotierDecisionLabelDto, AutotierDecisionListPage,
    AutotierDecisionQueryFilter, Database,
};
use crate::error::AppError;
use crate::store::AppState;

const DECISION_LABELS: [&str; 4] = [
    "correct",
    "should_be_stronger",
    "could_be_cheaper",
    "unsure",
];

const DECISION_LABEL_REASONS: [&str; 8] = [
    "tool_failure_risk",
    "long_context",
    "architecture_reasoning",
    "simple_formatting",
    "background_task",
    "wrong_provider_capability",
    "cache_risk",
    "other",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertDecisionLabelInput {
    pub decision_id: String,
    pub label: String,
    pub reason: Option<String>,
    pub note: Option<String>,
}

#[tauri::command]
pub fn autotier_query_decisions(
    state: State<'_, AppState>,
    filter: AutotierDecisionQueryFilter,
) -> Result<AutotierDecisionListPage, String> {
    query_decisions(&state.db, filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_get_decision_detail(
    state: State<'_, AppState>,
    decision_id: String,
) -> Result<Option<AutotierDecisionDetail>, String> {
    get_decision_detail(&state.db, &decision_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_upsert_decision_label(
    state: State<'_, AppState>,
    input: UpsertDecisionLabelInput,
) -> Result<AutotierDecisionLabelDto, String> {
    upsert_decision_label(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_get_decision_label(
    state: State<'_, AppState>,
    decision_id: String,
) -> Result<Option<AutotierDecisionLabelDto>, String> {
    get_decision_label(&state.db, &decision_id).map_err(|e| e.to_string())
}

pub(crate) fn query_decisions(
    db: &Database,
    filter: AutotierDecisionQueryFilter,
) -> Result<AutotierDecisionListPage, AppError> {
    db.autotier_query_decisions(&filter)
}

pub(crate) fn get_decision_detail(
    db: &Database,
    decision_id: &str,
) -> Result<Option<AutotierDecisionDetail>, AppError> {
    db.autotier_get_decision_detail(decision_id)
}

pub(crate) fn upsert_decision_label(
    db: &Database,
    input: UpsertDecisionLabelInput,
) -> Result<AutotierDecisionLabelDto, AppError> {
    let decision_id = input.decision_id.trim();
    if decision_id.is_empty() {
        return Err(AppError::InvalidInput("decision_id is required".into()));
    }
    if db.autotier_get_decision(decision_id)?.is_none() {
        return Err(AppError::InvalidInput(format!(
            "decision not found: {decision_id}"
        )));
    }
    let label = normalize_label(&input.label)?;
    let reason = normalize_optional_reason(input.reason.as_deref())?;
    let note = input
        .note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    let now = Utc::now().timestamp_millis();
    let dto = AutotierDecisionLabelDto {
        decision_id: decision_id.to_string(),
        label,
        reason,
        note,
        created_at: now,
        updated_at: now,
    };
    db.autotier_upsert_label(&dto)?;
    db.autotier_get_label(decision_id)?
        .ok_or_else(|| AppError::Database("label upsert did not persist".into()))
}

pub(crate) fn get_decision_label(
    db: &Database,
    decision_id: &str,
) -> Result<Option<AutotierDecisionLabelDto>, AppError> {
    if decision_id.trim().is_empty() {
        return Err(AppError::InvalidInput("decision_id is required".into()));
    }
    db.autotier_get_label(decision_id.trim())
}

fn normalize_label(raw: &str) -> Result<String, AppError> {
    let label = raw.trim().to_ascii_lowercase();
    if DECISION_LABELS.contains(&label.as_str()) {
        Ok(label)
    } else {
        Err(AppError::InvalidInput(format!(
            "illegal decision label: {label}"
        )))
    }
}

fn normalize_optional_reason(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(reason) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let reason = reason.to_ascii_lowercase();
    if DECISION_LABEL_REASONS.contains(&reason.as_str()) {
        Ok(Some(reason))
    } else {
        Err(AppError::InvalidInput(format!(
            "illegal decision label reason: {reason}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::AutotierDecisionRow;
    use crate::database::Database;

    fn test_db() -> Database {
        Database::memory().expect("memory db")
    }

    fn make_decision_row(id: &str) -> AutotierDecisionRow {
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
            vision_fallback_applied: false,
            vision_describe_input_tokens: None,
            vision_describe_output_tokens: None,
            upstream_message_id: None,
            usage_request_id: None,
            complexity_score: Some(0.2),
            confidence: Some(0.8),
            reason_codes_json: r#"["SHORT_USER_REQUEST"]"#.to_string(),
            unsafe_reasons_json: "[]".to_string(),
            safe_to_execute: false,
            feature_json: "{}".to_string(),
            feature_version: "claude-extractor-v0.2".to_string(),
            classifier_version: "rules-v0.2".to_string(),
            policy_version: "shadow-policy-v0.2".to_string(),
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
    fn upsert_label_requires_existing_decision() {
        let db = test_db();
        let err = upsert_decision_label(
            &db,
            UpsertDecisionLabelInput {
                decision_id: "missing".into(),
                label: "correct".into(),
                reason: None,
                note: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("decision not found"));
    }

    #[test]
    fn upsert_label_rejects_unknown_enum() {
        let db = test_db();
        db.autotier_upsert_decision(&make_decision_row("d-label"))
            .unwrap();
        let err = upsert_decision_label(
            &db,
            UpsertDecisionLabelInput {
                decision_id: "d-label".into(),
                label: "banana".into(),
                reason: None,
                note: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("illegal decision label"));
    }

    #[test]
    fn query_and_detail_round_trip() {
        let db = test_db();
        db.autotier_upsert_decision(&make_decision_row("d-round"))
            .unwrap();
        let page = query_decisions(
            &db,
            AutotierDecisionQueryFilter {
                limit: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 1);
        let detail = get_decision_detail(&db, "d-round").unwrap().unwrap();
        assert_eq!(detail.decision_id, "d-round");
        assert!(!detail.completion.usage_linked);
    }
}

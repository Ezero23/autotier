//! AutoTier decision list/detail queries with filters and pagination (Phase 7A).

use crate::database::{lock_conn, AutotierDecisionLabelDto, AutotierDecisionRow, Database};
use crate::error::AppError;
use rusqlite::{params_from_iter, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};

const MAX_PAGE_SIZE: i64 = 100;
const DEFAULT_PAGE_SIZE: i64 = 50;

/// List/detail filter input from commands.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutotierDecisionQueryFilter {
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub session_id_hash: Option<String>,
    pub app_type: Option<String>,
    pub client_requested_model: Option<String>,
    pub recommended_slot: Option<String>,
    pub candidate_model: Option<String>,
    pub actual_outbound_model: Option<String>,
    pub provider: Option<String>,
    pub reason_code: Option<String>,
    pub unsafe_reason: Option<String>,
    pub confidence_min: Option<f64>,
    pub confidence_max: Option<f64>,
    pub cache_protected: Option<bool>,
    pub is_complete: Option<bool>,
    pub label: Option<String>,
    pub has_label: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutotierDecisionCompletionStatus {
    pub decision_complete: bool,
    pub usage_linked: bool,
    pub missing_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutotierDecisionListItem {
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
    pub complexity_score: f64,
    pub confidence: f64,
    pub safe_to_execute: bool,
    pub is_complete: bool,
    pub error_code: Option<String>,
    pub user_label: Option<String>,
    pub completion: AutotierDecisionCompletionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutotierDecisionListPage {
    pub items: Vec<AutotierDecisionListItem>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutotierDecisionDetail {
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
    pub autotier_mutated_request: bool,
    pub upstream_message_id: Option<String>,
    pub usage_request_id: Option<String>,
    pub complexity_score: f64,
    pub confidence: f64,
    pub reason_codes_json: String,
    pub unsafe_reasons_json: String,
    pub safe_to_execute: bool,
    pub feature_json: String,
    pub feature_version: String,
    pub classifier_version: String,
    pub policy_version: String,
    pub actual_input_tokens: Option<i64>,
    pub actual_output_tokens: Option<i64>,
    pub actual_cache_read_tokens: Option<i64>,
    pub actual_cache_write_5m_tokens: Option<i64>,
    pub actual_cache_write_1h_tokens: Option<i64>,
    pub actual_cost_usd: Option<String>,
    pub candidate_cost_low_usd: Option<String>,
    pub candidate_cost_base_usd: Option<String>,
    pub candidate_cost_high_usd: Option<String>,
    pub cost_assumptions_json: String,
    pub status_code: Option<i64>,
    pub outcome: Option<String>,
    pub retry_count: i32,
    pub fallback_count: i32,
    pub is_complete: bool,
    pub error_code: Option<String>,
    pub user_label: Option<AutotierDecisionLabelDto>,
    pub completion: AutotierDecisionCompletionStatus,
}

pub fn normalize_page_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE)
}

pub fn completion_status(row: &AutotierDecisionRow) -> AutotierDecisionCompletionStatus {
    let usage_linked = row
        .usage_request_id
        .as_deref()
        .is_some_and(|id| !id.trim().is_empty())
        || row.actual_input_tokens.is_some()
        || row.actual_output_tokens.is_some();
    let mut missing_fields = Vec::new();
    if !row.is_complete {
        missing_fields.push("decision_finalize".to_string());
    }
    if row.completed_at.is_none() && row.is_complete {
        missing_fields.push("completed_at".to_string());
    }
    if !usage_linked {
        missing_fields.push("usage_link".to_string());
    }
    if row.baseline_outbound_model.is_none() {
        missing_fields.push("baseline_outbound_model".to_string());
    }
    if row.actual_outbound_model.is_none() {
        missing_fields.push("actual_outbound_model".to_string());
    }
    AutotierDecisionCompletionStatus {
        decision_complete: row.is_complete,
        usage_linked,
        missing_fields,
    }
}

fn list_item_from_row(
    row: AutotierDecisionRow,
    user_label: Option<String>,
) -> AutotierDecisionListItem {
    let completion = completion_status(&row);
    AutotierDecisionListItem {
        decision_id: row.decision_id,
        created_at: row.created_at,
        completed_at: row.completed_at,
        app_type: row.app_type,
        session_id_hash: row.session_id_hash,
        mode: row.mode,
        client_requested_model: row.client_requested_model,
        initial_selected_provider: row.initial_selected_provider,
        baseline_outbound_model: row.baseline_outbound_model,
        baseline_outbound_provider: row.baseline_outbound_provider,
        recommended_slot: row.recommended_slot,
        candidate_model: row.candidate_model,
        candidate_provider: row.candidate_provider,
        actual_outbound_model: row.actual_outbound_model,
        actual_outbound_provider: row.actual_outbound_provider,
        complexity_score: row.complexity_score.unwrap_or(0.0),
        confidence: row.confidence.unwrap_or(0.0),
        safe_to_execute: row.safe_to_execute,
        is_complete: row.is_complete,
        error_code: row.error_code,
        user_label,
        completion,
    }
}

fn detail_from_row(
    row: AutotierDecisionRow,
    user_label: Option<AutotierDecisionLabelDto>,
) -> AutotierDecisionDetail {
    let completion = completion_status(&row);
    AutotierDecisionDetail {
        decision_id: row.decision_id,
        created_at: row.created_at,
        completed_at: row.completed_at,
        app_type: row.app_type,
        session_id_hash: row.session_id_hash,
        mode: row.mode,
        client_requested_model: row.client_requested_model,
        initial_selected_provider: row.initial_selected_provider,
        baseline_outbound_model: row.baseline_outbound_model,
        baseline_outbound_provider: row.baseline_outbound_provider,
        recommended_slot: row.recommended_slot,
        candidate_model: row.candidate_model,
        candidate_provider: row.candidate_provider,
        actual_outbound_model: row.actual_outbound_model,
        actual_outbound_provider: row.actual_outbound_provider,
        autotier_mutated_request: row.autotier_mutated_request,
        upstream_message_id: row.upstream_message_id,
        usage_request_id: row.usage_request_id,
        complexity_score: row.complexity_score.unwrap_or(0.0),
        confidence: row.confidence.unwrap_or(0.0),
        reason_codes_json: row.reason_codes_json,
        unsafe_reasons_json: row.unsafe_reasons_json,
        safe_to_execute: row.safe_to_execute,
        feature_json: row.feature_json,
        feature_version: row.feature_version,
        classifier_version: row.classifier_version,
        policy_version: row.policy_version,
        actual_input_tokens: row.actual_input_tokens,
        actual_output_tokens: row.actual_output_tokens,
        actual_cache_read_tokens: row.actual_cache_read_tokens,
        actual_cache_write_5m_tokens: row.actual_cache_write_5m_tokens,
        actual_cache_write_1h_tokens: row.actual_cache_write_1h_tokens,
        actual_cost_usd: row.actual_cost_usd,
        candidate_cost_low_usd: row.candidate_cost_low_usd,
        candidate_cost_base_usd: row.candidate_cost_base_usd,
        candidate_cost_high_usd: row.candidate_cost_high_usd,
        cost_assumptions_json: row.cost_assumptions_json,
        status_code: row.status_code,
        outcome: row.outcome,
        retry_count: row.retry_count,
        fallback_count: row.fallback_count,
        is_complete: row.is_complete,
        error_code: row.error_code,
        user_label,
        completion,
    }
}

struct QueryParts {
    where_sql: String,
    params: Vec<Box<dyn ToSql>>,
}

fn escaped_like_pattern(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn push_like(parts: &mut QueryParts, column: &str, value: &str) {
    parts
        .where_sql
        .push_str(&format!(" AND {column} LIKE ? ESCAPE '\\'"));
    parts.params.push(Box::new(escaped_like_pattern(value)));
}

fn build_query_parts(filter: &AutotierDecisionQueryFilter) -> QueryParts {
    let mut parts = QueryParts {
        where_sql: String::from("WHERE 1=1"),
        params: Vec::new(),
    };
    if let Some(since) = filter.since_ms {
        parts.where_sql.push_str(" AND d.created_at >= ?");
        parts.params.push(Box::new(since));
    }
    if let Some(until) = filter.until_ms {
        parts.where_sql.push_str(" AND d.created_at < ?");
        parts.params.push(Box::new(until));
    }
    if let Some(session) = filter
        .session_id_hash
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        parts.where_sql.push_str(" AND d.session_id_hash = ?");
        parts.params.push(Box::new(session.to_string()));
    }
    if let Some(app) = filter
        .app_type
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
    {
        parts.where_sql.push_str(" AND d.app_type = ?");
        parts.params.push(Box::new(app));
    }
    if let Some(model) = filter
        .client_requested_model
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        push_like(&mut parts, "d.client_requested_model", model.trim());
    }
    if let Some(slot) = filter
        .recommended_slot
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
    {
        parts.where_sql.push_str(" AND d.recommended_slot = ?");
        parts.params.push(Box::new(slot));
    }
    if let Some(model) = filter
        .candidate_model
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        push_like(&mut parts, "d.candidate_model", model.trim());
    }
    if let Some(model) = filter
        .actual_outbound_model
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        push_like(&mut parts, "d.actual_outbound_model", model.trim());
    }
    if let Some(provider) = filter
        .provider
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let pattern = escaped_like_pattern(provider);
        parts.where_sql.push_str(
            " AND (d.initial_selected_provider LIKE ? ESCAPE '\\'
                OR d.baseline_outbound_provider LIKE ? ESCAPE '\\'
                OR d.candidate_provider LIKE ? ESCAPE '\\'
                OR d.actual_outbound_provider LIKE ? ESCAPE '\\')",
        );
        for _ in 0..4 {
            parts.params.push(Box::new(pattern.clone()));
        }
    }
    if let Some(reason) = filter
        .reason_code
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        push_like(&mut parts, "d.reason_codes_json", reason);
    }
    if let Some(reason) = filter
        .unsafe_reason
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        push_like(&mut parts, "d.unsafe_reasons_json", reason);
    }
    if let Some(min) = filter.confidence_min {
        parts
            .where_sql
            .push_str(" AND COALESCE(d.confidence, 0) >= ?");
        parts.params.push(Box::new(min));
    }
    if let Some(max) = filter.confidence_max {
        parts
            .where_sql
            .push_str(" AND COALESCE(d.confidence, 0) <= ?");
        parts.params.push(Box::new(max));
    }
    if let Some(cache_protected) = filter.cache_protected {
        let cache_sql = "(d.reason_codes_json LIKE '%CACHE%' OR d.unsafe_reasons_json LIKE '%CACHE%' OR d.actual_cache_read_tokens IS NOT NULL OR d.actual_cache_write_5m_tokens IS NOT NULL OR d.actual_cache_write_1h_tokens IS NOT NULL)";
        parts.where_sql.push_str(if cache_protected {
            " AND "
        } else {
            " AND NOT "
        });
        parts.where_sql.push_str(cache_sql);
    }
    if let Some(complete) = filter.is_complete {
        parts.where_sql.push_str(" AND d.is_complete = ?");
        parts
            .params
            .push(Box::new(if complete { 1i64 } else { 0i64 }));
    }
    if let Some(label) = filter
        .label
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
    {
        parts.where_sql.push_str(" AND l.label = ?");
        parts.params.push(Box::new(label));
    }
    if let Some(has_label) = filter.has_label {
        if has_label {
            parts.where_sql.push_str(" AND l.decision_id IS NOT NULL");
        } else {
            parts.where_sql.push_str(" AND l.decision_id IS NULL");
        }
    }
    parts
}

fn map_decision_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(AutotierDecisionRow, Option<String>)> {
    Ok((
        AutotierDecisionRow {
            decision_id: row.get(0)?,
            created_at: row.get(1)?,
            completed_at: row.get(2)?,
            app_type: row.get(3)?,
            session_id_hash: row.get(4)?,
            mode: row.get(5)?,
            client_requested_model: row.get(6)?,
            initial_selected_provider: row.get(7)?,
            baseline_outbound_model: row.get(8)?,
            baseline_outbound_provider: row.get(9)?,
            recommended_slot: row.get(10)?,
            candidate_model: row.get(11)?,
            candidate_provider: row.get(12)?,
            actual_outbound_model: row.get(13)?,
            actual_outbound_provider: row.get(14)?,
            autotier_mutated_request: row.get(15)?,
            upstream_message_id: row.get(16)?,
            usage_request_id: row.get(17)?,
            complexity_score: row.get(18)?,
            confidence: row.get(19)?,
            reason_codes_json: row.get(20)?,
            unsafe_reasons_json: row.get(21)?,
            safe_to_execute: row.get(22)?,
            feature_json: row.get(23)?,
            feature_version: row.get(24)?,
            classifier_version: row.get(25)?,
            policy_version: row.get(26)?,
            actual_input_tokens: row.get(27)?,
            actual_output_tokens: row.get(28)?,
            actual_cache_read_tokens: row.get(29)?,
            actual_cache_write_5m_tokens: row.get(30)?,
            actual_cache_write_1h_tokens: row.get(31)?,
            actual_cost_usd: row.get(32)?,
            candidate_cost_low_usd: row.get(33)?,
            candidate_cost_base_usd: row.get(34)?,
            candidate_cost_high_usd: row.get(35)?,
            cost_assumptions_json: row.get(36)?,
            status_code: row.get(37)?,
            outcome: row.get(38)?,
            retry_count: row.get(39)?,
            fallback_count: row.get(40)?,
            is_complete: row.get(41)?,
            error_code: row.get(42)?,
        },
        row.get(43)?,
    ))
}

const DECISION_SELECT: &str = "SELECT d.decision_id, d.created_at, d.completed_at, d.app_type, d.session_id_hash, d.mode,
                        d.client_requested_model, d.initial_selected_provider,
                        d.baseline_outbound_model, d.baseline_outbound_provider,
                        d.recommended_slot, d.candidate_model, d.candidate_provider,
                        d.actual_outbound_model, d.actual_outbound_provider,
                        d.autotier_mutated_request,
                        d.upstream_message_id, d.usage_request_id,
                        d.complexity_score, d.confidence,
                        d.reason_codes_json, d.unsafe_reasons_json, d.safe_to_execute,
                        d.feature_json, d.feature_version, d.classifier_version, d.policy_version,
                        d.actual_input_tokens, d.actual_output_tokens,
                        d.actual_cache_read_tokens, d.actual_cache_write_5m_tokens, d.actual_cache_write_1h_tokens,
                        d.actual_cost_usd,
                        d.candidate_cost_low_usd, d.candidate_cost_base_usd, d.candidate_cost_high_usd,
                        d.cost_assumptions_json,
                        d.status_code, d.outcome, d.retry_count, d.fallback_count,
                        d.is_complete, d.error_code,
                        l.label AS user_label";

impl Database {
    pub fn autotier_query_decisions(
        &self,
        filter: &AutotierDecisionQueryFilter,
    ) -> Result<AutotierDecisionListPage, AppError> {
        let limit = normalize_page_limit(filter.limit);
        let offset = filter.offset.unwrap_or(0).max(0);
        let parts = build_query_parts(filter);
        let conn = lock_conn!(self.conn);

        let count_sql = format!(
            "SELECT COUNT(*) FROM autotier_routing_decisions d
             LEFT JOIN autotier_decision_labels l ON l.decision_id = d.decision_id
             {}",
            parts.where_sql
        );
        let total: i64 = conn
            .query_row(
                &count_sql,
                params_from_iter(parts.params.iter().map(|p| p.as_ref())),
                |row| row.get(0),
            )
            .map_err(|e| {
                AppError::Database(format!("autotier_query_decisions count failed: {e}"))
            })?;

        let list_sql = format!(
            "{DECISION_SELECT}
             FROM autotier_routing_decisions d
             LEFT JOIN autotier_decision_labels l ON l.decision_id = d.decision_id
             {}
             ORDER BY d.created_at DESC
             LIMIT ? OFFSET ?",
            parts.where_sql
        );
        let mut list_params: Vec<Box<dyn ToSql>> = parts.params;
        list_params.push(Box::new(limit));
        list_params.push(Box::new(offset));

        let mut stmt = conn.prepare(&list_sql).map_err(|e| {
            AppError::Database(format!("autotier_query_decisions prepare failed: {e}"))
        })?;
        let rows = stmt
            .query_map(
                params_from_iter(list_params.iter().map(|p| p.as_ref())),
                map_decision_row,
            )
            .map_err(|e| AppError::Database(format!("autotier_query_decisions query failed: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("autotier_query_decisions row failed: {e}")))?;

        let items = rows
            .into_iter()
            .map(|(row, label)| list_item_from_row(row, label))
            .collect();

        Ok(AutotierDecisionListPage {
            items,
            total,
            limit,
            offset,
        })
    }

    pub fn autotier_get_decision_detail(
        &self,
        decision_id: &str,
    ) -> Result<Option<AutotierDecisionDetail>, AppError> {
        if decision_id.trim().is_empty() {
            return Err(AppError::InvalidInput("decision_id is required".into()));
        }
        let decision = {
            let conn = lock_conn!(self.conn);
            let sql = format!(
                "{DECISION_SELECT}
                 FROM autotier_routing_decisions d
                 LEFT JOIN autotier_decision_labels l ON l.decision_id = d.decision_id
                 WHERE d.decision_id = ?1"
            );
            conn.query_row(&sql, [decision_id], map_decision_row)
                .optional()
                .map_err(|e| {
                    AppError::Database(format!("autotier_get_decision_detail failed: {e}"))
                })?
                .map(|(decision, _label)| decision)
        };
        let Some(decision) = decision else {
            return Ok(None);
        };
        let user_label = self.autotier_get_label(decision_id)?;
        Ok(Some(detail_from_row(decision, user_label)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn query_decisions_filters_and_paginates() {
        let db = test_db();
        for i in 0..5 {
            let mut row = make_decision_row(&format!("d-q-{i}"));
            row.created_at = 10_000 + i;
            row.app_type = if i % 2 == 0 {
                "claude".into()
            } else {
                "codex".into()
            };
            row.recommended_slot = Some("cheap".into());
            row.is_complete = i % 2 == 0;
            db.autotier_upsert_decision(&row).unwrap();
        }

        let page = db
            .autotier_query_decisions(&AutotierDecisionQueryFilter {
                app_type: Some("claude".into()),
                limit: Some(2),
                offset: Some(0),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert!(page.items.iter().all(|item| item.app_type == "claude"));

        let incomplete = db
            .autotier_query_decisions(&AutotierDecisionQueryFilter {
                is_complete: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(incomplete.total, 2);
        assert!(incomplete.items.iter().all(|item| !item.is_complete));
    }

    #[test]
    fn detail_includes_completion_and_label() {
        let db = test_db();
        let mut row = make_decision_row("d-detail");
        row.usage_request_id = Some("usage-1".into());
        row.is_complete = true;
        row.completed_at = Some(20_000);
        db.autotier_upsert_decision(&row).unwrap();
        db.autotier_upsert_label(&AutotierDecisionLabelDto {
            decision_id: "d-detail".into(),
            label: "correct".into(),
            reason: Some("other".into()),
            note: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();

        let detail = db
            .autotier_get_decision_detail("d-detail")
            .unwrap()
            .unwrap();
        assert_eq!(detail.decision_id, "d-detail");
        assert!(detail.completion.decision_complete);
        assert!(detail.completion.usage_linked);
        assert_eq!(
            detail.user_label.as_ref().map(|l| l.label.as_str()),
            Some("correct")
        );
    }

    #[test]
    fn clear_cascades_labels() {
        let db = test_db();
        db.autotier_upsert_decision(&make_decision_row("d-cascade"))
            .unwrap();
        db.autotier_upsert_label(&AutotierDecisionLabelDto {
            decision_id: "d-cascade".into(),
            label: "unsure".into(),
            reason: None,
            note: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
        db.autotier_clear_all_decisions().unwrap();
        assert_eq!(db.autotier_count_decisions().unwrap(), 0);
        assert_eq!(db.autotier_count_labels().unwrap(), 0);
    }

    #[test]
    fn query_filters_provider_reason_confidence_and_cache() {
        let db = test_db();
        let mut cached = make_decision_row("d-filter-cached");
        cached.initial_selected_provider = Some("provider-a".into());
        cached.candidate_provider = Some("provider-a".into());
        cached.reason_codes_json = r#"["CACHE_PROTECTED"]"#.into();
        cached.confidence = Some(0.9);
        cached.actual_cache_read_tokens = Some(12);
        db.autotier_upsert_decision(&cached).unwrap();

        let mut uncached = make_decision_row("d-filter-uncached");
        uncached.initial_selected_provider = Some("provider-b".into());
        uncached.candidate_provider = Some("provider-b".into());
        uncached.reason_codes_json = r#"["SHORT_USER_REQUEST"]"#.into();
        uncached.confidence = Some(0.3);
        db.autotier_upsert_decision(&uncached).unwrap();

        let page = db
            .autotier_query_decisions(&AutotierDecisionQueryFilter {
                provider: Some("provider-a".into()),
                reason_code: Some("CACHE_PROTECTED".into()),
                confidence_min: Some(0.8),
                cache_protected: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].decision_id, "d-filter-cached");

        let page = db
            .autotier_query_decisions(&AutotierDecisionQueryFilter {
                confidence_max: Some(0.5),
                cache_protected: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].decision_id, "d-filter-uncached");
    }
}

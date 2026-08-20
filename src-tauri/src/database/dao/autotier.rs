//! AutoTier DAO — 决策日志、配置、Slot 与标注的数据访问层
//!
//! 管理四张 `autotier_*` 表的读写操作：
//! - `autotier_provider_slots`：Provider-specific 槽位映射
//! - `autotier_routing_config`：路由模式与保留策略（单行）
//! - `autotier_routing_decisions`：逐请求决策日志
//! - `autotier_decision_labels`：用户标注
//!
//! 所有方法通过 `impl Database` 提供，遵循基座 DAO 模式（`lock_conn!` 宏）。
//! 不修改现有 `proxy_request_logs` 或其他基座表。

use crate::autotier::{CLASSIFIER_VERSION, FEATURE_VERSION, POLICY_VERSION};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DTO 类型
// ---------------------------------------------------------------------------

/// `autotier_routing_config` 单行 DTO（PRD §11.2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutotierRoutingConfigDto {
    pub mode: String,
    pub retention_days: i32,
    pub raw_prompt_opt_in: bool,
    pub classifier_version: String,
    pub feature_version: String,
    pub policy_version: String,
    pub updated_at: i64,
}

impl Default for AutotierRoutingConfigDto {
    fn default() -> Self {
        Self {
            mode: "shadow".to_string(),
            retention_days: 30,
            raw_prompt_opt_in: false,
            classifier_version: CLASSIFIER_VERSION.to_string(),
            feature_version: FEATURE_VERSION.to_string(),
            policy_version: POLICY_VERSION.to_string(),
            updated_at: 0,
        }
    }
}

/// `autotier_provider_slots` 行 DTO（PRD §11.1）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutotierProviderSlotDto {
    pub provider_id: String,
    pub slot: String,
    pub model_id: String,
    pub capability_status: String,
    pub supports_tools: Option<bool>,
    pub supports_streaming: Option<bool>,
    pub supports_vision: Option<bool>,
    pub context_limit: Option<i64>,
    pub api_format: Option<String>,
    pub pricing_source: Option<String>,
    pub capability_source: Option<String>,
    pub verified_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// `provider_model_pricing` Provider-specific 定价快照行 DTO。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutotierProviderModelPricingDto {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
    pub price_source: String,
    pub observed_at: i64,
}

/// Provider-specific 定价快照写入输入，不允许前端伪造 observed_at。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertAutotierProviderModelPricingInput {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
    pub price_source: Option<String>,
}

/// `autotier_routing_decisions` 行 DTO（PRD §11.3）。
///
/// 用于插入和查询决策记录。JSON 字段以字符串存储。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutotierDecisionRow {
    pub decision_id: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub app_type: String,
    pub session_id_hash: String,
    pub mode: String,

    // 客户端请求组
    pub client_requested_model: String,
    pub initial_selected_provider: Option<String>,

    // 基线出站组
    pub baseline_outbound_model: Option<String>,
    pub baseline_outbound_provider: Option<String>,

    // 候选组
    pub recommended_slot: Option<String>,
    pub candidate_model: Option<String>,
    pub candidate_provider: Option<String>,

    // 实际出站组
    pub actual_outbound_model: Option<String>,
    pub actual_outbound_provider: Option<String>,

    // Shadow 不变量标志
    pub autotier_mutated_request: bool,

    // 关联 ID
    pub upstream_message_id: Option<String>,
    pub usage_request_id: Option<String>,

    // 决策引擎输出
    pub complexity_score: Option<f64>,
    pub confidence: Option<f64>,
    pub reason_codes_json: String,
    pub unsafe_reasons_json: String,
    pub safe_to_execute: bool,

    // 版本戳
    pub feature_json: String,
    pub feature_version: String,
    pub classifier_version: String,
    pub policy_version: String,

    // Usage
    pub actual_input_tokens: Option<i64>,
    pub actual_output_tokens: Option<i64>,
    pub actual_cache_read_tokens: Option<i64>,
    pub actual_cache_write_5m_tokens: Option<i64>,
    pub actual_cache_write_1h_tokens: Option<i64>,
    pub actual_cost_usd: Option<String>,

    // 候选成本投影
    pub candidate_cost_low_usd: Option<String>,
    pub candidate_cost_base_usd: Option<String>,
    pub candidate_cost_high_usd: Option<String>,
    pub cost_assumptions_json: String,

    // 状态
    pub status_code: Option<i64>,
    pub outcome: Option<String>,
    pub retry_count: i32,
    pub fallback_count: i32,
    pub is_complete: bool,
    pub error_code: Option<String>,
}

/// `autotier_decision_labels` 行 DTO（PRD §11.4）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutotierDecisionLabelDto {
    pub decision_id: String,
    pub label: String,
    pub reason: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------
// Provider-specific pricing DAO
// ---------------------------------------------------------------------------

impl Database {
    pub fn autotier_list_provider_model_pricing(
        &self,
        provider_id: &str,
    ) -> Result<Vec<AutotierProviderModelPricingDto>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, model_id, display_name,
                        input_cost_per_million, output_cost_per_million,
                        cache_read_cost_per_million, cache_creation_cost_per_million,
                        price_source, observed_at
                 FROM provider_model_pricing
                 WHERE provider_id = ?1
                 ORDER BY model_id COLLATE NOCASE",
            )
            .map_err(|e| AppError::Database(format!("list provider pricing failed: {e}")))?;
        let rows = stmt
            .query_map(params![provider_id], |row| {
                Ok(AutotierProviderModelPricingDto {
                    provider_id: row.get(0)?,
                    model_id: row.get(1)?,
                    display_name: row.get(2)?,
                    input_cost_per_million: row.get(3)?,
                    output_cost_per_million: row.get(4)?,
                    cache_read_cost_per_million: row.get(5)?,
                    cache_creation_cost_per_million: row.get(6)?,
                    price_source: row.get(7)?,
                    observed_at: row.get(8)?,
                })
            })
            .map_err(|e| AppError::Database(format!("read provider pricing failed: {e}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("decode provider pricing failed: {e}")))
    }

    pub fn autotier_upsert_provider_model_pricing(
        &self,
        row: &AutotierProviderModelPricingDto,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO provider_model_pricing (
                provider_id, model_id, display_name,
                input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million,
                price_source, observed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(provider_id, model_id) DO UPDATE SET
                display_name = excluded.display_name,
                input_cost_per_million = excluded.input_cost_per_million,
                output_cost_per_million = excluded.output_cost_per_million,
                cache_read_cost_per_million = excluded.cache_read_cost_per_million,
                cache_creation_cost_per_million = excluded.cache_creation_cost_per_million,
                price_source = excluded.price_source,
                observed_at = excluded.observed_at",
            params![
                row.provider_id,
                row.model_id,
                row.display_name,
                row.input_cost_per_million,
                row.output_cost_per_million,
                row.cache_read_cost_per_million,
                row.cache_creation_cost_per_million,
                row.price_source,
                row.observed_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("upsert provider pricing failed: {e}")))?;
        Ok(())
    }

    pub fn autotier_delete_provider_model_pricing(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        let deleted = conn
            .execute(
                "DELETE FROM provider_model_pricing WHERE provider_id = ?1 AND model_id = ?2",
                params![provider_id, model_id],
            )
            .map_err(|e| AppError::Database(format!("delete provider pricing failed: {e}")))?;
        Ok(deleted as u64)
    }
}

// ---------------------------------------------------------------------------
// Decision DAO
// ---------------------------------------------------------------------------

impl Database {
    /// 插入一条新的决策记录。
    ///
    /// 同一 `decision_id` 重复调用会静默跳过（`ON CONFLICT DO NOTHING`），
    /// 避免覆盖已有决策或级联删除用户标注（PRD §FR-DATA-001）。
    /// 如需在 Finalize 后回填字段，请使用 `autotier_finalize_decision`。
    pub fn autotier_upsert_decision(&self, row: &AutotierDecisionRow) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO autotier_routing_decisions (
                decision_id, created_at, completed_at, app_type, session_id_hash, mode,
                client_requested_model, initial_selected_provider,
                baseline_outbound_model, baseline_outbound_provider,
                recommended_slot, candidate_model, candidate_provider,
                actual_outbound_model, actual_outbound_provider,
                autotier_mutated_request,
                upstream_message_id, usage_request_id,
                complexity_score, confidence,
                reason_codes_json, unsafe_reasons_json, safe_to_execute,
                feature_json, feature_version, classifier_version, policy_version,
                actual_input_tokens, actual_output_tokens,
                actual_cache_read_tokens, actual_cache_write_5m_tokens, actual_cache_write_1h_tokens,
                actual_cost_usd,
                candidate_cost_low_usd, candidate_cost_base_usd, candidate_cost_high_usd,
                cost_assumptions_json,
                status_code, outcome, retry_count, fallback_count,
                is_complete, error_code
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8,
                ?9, ?10,
                ?11, ?12, ?13,
                ?14, ?15,
                ?16,
                ?17, ?18,
                ?19, ?20,
                ?21, ?22, ?23,
                ?24, ?25, ?26, ?27,
                ?28, ?29,
                ?30, ?31, ?32,
                ?33,
                ?34, ?35, ?36,
                ?37,
                ?38, ?39, ?40, ?41,
                ?42, ?43
            ) ON CONFLICT(decision_id) DO NOTHING",
            params![
                row.decision_id,
                row.created_at,
                row.completed_at,
                row.app_type,
                row.session_id_hash,
                row.mode,
                row.client_requested_model,
                row.initial_selected_provider,
                row.baseline_outbound_model,
                row.baseline_outbound_provider,
                row.recommended_slot,
                row.candidate_model,
                row.candidate_provider,
                row.actual_outbound_model,
                row.actual_outbound_provider,
                row.autotier_mutated_request,
                row.upstream_message_id,
                row.usage_request_id,
                row.complexity_score,
                row.confidence,
                row.reason_codes_json,
                row.unsafe_reasons_json,
                row.safe_to_execute,
                row.feature_json,
                row.feature_version,
                row.classifier_version,
                row.policy_version,
                row.actual_input_tokens,
                row.actual_output_tokens,
                row.actual_cache_read_tokens,
                row.actual_cache_write_5m_tokens,
                row.actual_cache_write_1h_tokens,
                row.actual_cost_usd,
                row.candidate_cost_low_usd,
                row.candidate_cost_base_usd,
                row.candidate_cost_high_usd,
                row.cost_assumptions_json,
                row.status_code,
                row.outcome,
                row.retry_count,
                row.fallback_count,
                row.is_complete,
                row.error_code,
            ],
        )
        .map_err(|e| AppError::Database(format!("autotier_upsert_decision failed: {e}")))?;
        Ok(())
    }
}

/// `autotier_finalize_decision` 的参数集。
///
/// 使用结构体避免过多参数；`None` 字段表示"不更新"（COALESCE 保留原值）。
#[derive(Debug, Clone, Default)]
pub struct FinalizeDecisionParams<'a> {
    pub decision_id: &'a str,
    pub completed_at: i64,
    pub actual_outbound_model: Option<&'a str>,
    pub actual_outbound_provider: Option<&'a str>,
    pub upstream_message_id: Option<&'a str>,
    pub usage_request_id: Option<&'a str>,
    pub actual_input_tokens: Option<i64>,
    pub actual_output_tokens: Option<i64>,
    pub actual_cache_read_tokens: Option<i64>,
    pub actual_cache_write_5m_tokens: Option<i64>,
    pub actual_cache_write_1h_tokens: Option<i64>,
    pub actual_cost_usd: Option<&'a str>,
    pub status_code: Option<i64>,
    pub outcome: Option<&'a str>,
    pub retry_count: Option<i32>,
    pub fallback_count: Option<i32>,
    pub error_code: Option<&'a str>,
}

impl Database {
    /// Finalize：回填 usage、实际出站、关联 ID。
    ///
    /// PRD §11.0：从同一个 RequestContext 取 decision_id 直接 UPDATE，
    /// 不做数据库反查。只更新提供的非 None 字段（COALESCE 保留原值）。
    ///
    /// `is_complete` 仅在关键 usage 字段（`usage_request_id` 或 token 计数）
    /// 非空时设为 true；无 usage 数据时不标记 complete（PRD §11.0）。
    ///
    /// 返回 `Err` 当 `decision_id` 不存在（affected_rows == 0），
    /// 避免调用方误以为已收口。
    pub fn autotier_finalize_decision(
        &self,
        params: &FinalizeDecisionParams<'_>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // Completion predicate: 至少有 usage_request_id 或 token 计数才认为完整。
        let has_usage = params.usage_request_id.is_some()
            || params.actual_input_tokens.is_some()
            || params.actual_output_tokens.is_some();

        let result = conn.execute(
            "UPDATE autotier_routing_decisions SET
                completed_at = ?2,
                actual_outbound_model = COALESCE(?3, actual_outbound_model),
                actual_outbound_provider = COALESCE(?4, actual_outbound_provider),
                upstream_message_id = COALESCE(?5, upstream_message_id),
                usage_request_id = COALESCE(?6, usage_request_id),
                actual_input_tokens = COALESCE(?7, actual_input_tokens),
                actual_output_tokens = COALESCE(?8, actual_output_tokens),
                actual_cache_read_tokens = COALESCE(?9, actual_cache_read_tokens),
                actual_cache_write_5m_tokens = COALESCE(?10, actual_cache_write_5m_tokens),
                actual_cache_write_1h_tokens = COALESCE(?11, actual_cache_write_1h_tokens),
                actual_cost_usd = COALESCE(?12, actual_cost_usd),
                status_code = COALESCE(?13, status_code),
                outcome = COALESCE(?14, outcome),
                retry_count = COALESCE(?15, retry_count),
                fallback_count = COALESCE(?16, fallback_count),
                error_code = COALESCE(?17, error_code),
                is_complete = CASE WHEN ?18 = 1 THEN 1 ELSE is_complete END
             WHERE decision_id = ?1",
            params![
                params.decision_id,
                params.completed_at,
                params.actual_outbound_model,
                params.actual_outbound_provider,
                params.upstream_message_id,
                params.usage_request_id,
                params.actual_input_tokens,
                params.actual_output_tokens,
                params.actual_cache_read_tokens,
                params.actual_cache_write_5m_tokens,
                params.actual_cache_write_1h_tokens,
                params.actual_cost_usd,
                params.status_code,
                params.outcome,
                params.retry_count,
                params.fallback_count,
                params.error_code,
                if has_usage { 1 } else { 0 },
            ],
        );
        let affected = result
            .map_err(|e| AppError::Database(format!("autotier_finalize_decision failed: {e}")))?;
        if affected == 0 {
            return Err(AppError::Database(format!(
                "autotier_finalize_decision: decision_id '{}' not found",
                params.decision_id
            )));
        }
        Ok(())
    }

    /// 按 decision_id 查询单条决策。
    pub fn autotier_get_decision(
        &self,
        decision_id: &str,
    ) -> Result<Option<AutotierDecisionRow>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT decision_id, created_at, completed_at, app_type, session_id_hash, mode,
                        client_requested_model, initial_selected_provider,
                        baseline_outbound_model, baseline_outbound_provider,
                        recommended_slot, candidate_model, candidate_provider,
                        actual_outbound_model, actual_outbound_provider,
                        autotier_mutated_request,
                        upstream_message_id, usage_request_id,
                        complexity_score, confidence,
                        reason_codes_json, unsafe_reasons_json, safe_to_execute,
                        feature_json, feature_version, classifier_version, policy_version,
                        actual_input_tokens, actual_output_tokens,
                        actual_cache_read_tokens, actual_cache_write_5m_tokens, actual_cache_write_1h_tokens,
                        actual_cost_usd,
                        candidate_cost_low_usd, candidate_cost_base_usd, candidate_cost_high_usd,
                        cost_assumptions_json,
                        status_code, outcome, retry_count, fallback_count,
                        is_complete, error_code
                 FROM autotier_routing_decisions
                 WHERE decision_id = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let row = stmt
            .query_row([decision_id], |row| {
                Ok(AutotierDecisionRow {
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
                })
            })
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(row)
    }

    /// 按时间范围查询决策（分页）。
    pub fn autotier_list_decisions(
        &self,
        since: i64,
        until: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AutotierDecisionRow>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT decision_id, created_at, completed_at, app_type, session_id_hash, mode,
                        client_requested_model, initial_selected_provider,
                        baseline_outbound_model, baseline_outbound_provider,
                        recommended_slot, candidate_model, candidate_provider,
                        actual_outbound_model, actual_outbound_provider,
                        autotier_mutated_request,
                        upstream_message_id, usage_request_id,
                        complexity_score, confidence,
                        reason_codes_json, unsafe_reasons_json, safe_to_execute,
                        feature_json, feature_version, classifier_version, policy_version,
                        actual_input_tokens, actual_output_tokens,
                        actual_cache_read_tokens, actual_cache_write_5m_tokens, actual_cache_write_1h_tokens,
                        actual_cost_usd,
                        candidate_cost_low_usd, candidate_cost_base_usd, candidate_cost_high_usd,
                        cost_assumptions_json,
                        status_code, outcome, retry_count, fallback_count,
                        is_complete, error_code
                 FROM autotier_routing_decisions
                 WHERE created_at >= ?1 AND created_at < ?2
                 ORDER BY created_at DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![since, until, limit, offset], |row| {
                Ok(AutotierDecisionRow {
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
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(rows)
    }

    /// 按保留策略删除过期决策（PRD §FR-DATA-002）。
    ///
    /// `retention_days = 0` 表示仅内存、不持久化（调用方应在 0 时不写入）。
    /// 返回删除的行数。
    pub fn autotier_prune_decisions(&self, retention_days: i32) -> Result<u64, AppError> {
        if retention_days <= 0 {
            // 0 天保留 = 不持久化，但已存在的行应全部清除
            let conn = lock_conn!(self.conn);
            let count = conn
                .execute("DELETE FROM autotier_routing_decisions", [])
                .map_err(|e| AppError::Database(format!("autotier_prune_decisions failed: {e}")))?;
            return Ok(count as u64);
        }
        let cutoff = chrono::Local::now().timestamp() - (retention_days as i64) * 86_400;
        let conn = lock_conn!(self.conn);
        let count = conn
            .execute(
                "DELETE FROM autotier_routing_decisions WHERE created_at < ?1",
                params![cutoff],
            )
            .map_err(|e| AppError::Database(format!("autotier_prune_decisions failed: {e}")))?;
        Ok(count as u64)
    }

    /// 清除所有 AutoTier 决策和标注（PRD §FR-DATA-002、§6.5）。
    ///
    /// 不删除基座 `proxy_request_logs` 数据。
    /// labels 通过外键 CASCADE 自动删除。
    pub fn autotier_clear_all_decisions(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM autotier_routing_decisions", [])
            .map_err(|e| AppError::Database(format!("autotier_clear_all_decisions failed: {e}")))?;
        Ok(())
    }

    /// 统计决策总数（用于 UI 和门禁检查）。
    pub fn autotier_count_decisions(&self) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM autotier_routing_decisions",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count)
    }

    /// 统计独立 Session 数（用于 Live Gate 检查）。
    pub fn autotier_count_sessions(&self) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT session_id_hash) FROM autotier_routing_decisions",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Label DAO
// ---------------------------------------------------------------------------

impl Database {
    /// 插入或更新标注（幂等，按 decision_id 主键）。
    pub fn autotier_upsert_label(&self, label: &AutotierDecisionLabelDto) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO autotier_decision_labels (
                decision_id, label, reason, note, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(decision_id) DO UPDATE SET
                label = excluded.label,
                reason = excluded.reason,
                note = excluded.note,
                updated_at = excluded.updated_at",
            params![
                label.decision_id,
                label.label,
                label.reason,
                label.note,
                label.created_at,
                label.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("autotier_upsert_label failed: {e}")))?;
        Ok(())
    }

    /// 查询某决策的标注。
    pub fn autotier_get_label(
        &self,
        decision_id: &str,
    ) -> Result<Option<AutotierDecisionLabelDto>, AppError> {
        let conn = lock_conn!(self.conn);
        let row = conn
            .query_row(
                "SELECT decision_id, label, reason, note, created_at, updated_at
                 FROM autotier_decision_labels
                 WHERE decision_id = ?1",
                [decision_id],
                |row| {
                    Ok(AutotierDecisionLabelDto {
                        decision_id: row.get(0)?,
                        label: row.get(1)?,
                        reason: row.get(2)?,
                        note: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(row)
    }

    /// 统计标注总数（用于 Live Gate 检查）。
    pub fn autotier_count_labels(&self) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM autotier_decision_labels", [], |row| {
                row.get(0)
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count)
    }

    /// 清除所有标注。
    pub fn autotier_clear_all_labels(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM autotier_decision_labels", [])
            .map_err(|e| AppError::Database(format!("autotier_clear_all_labels failed: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Config DAO
// ---------------------------------------------------------------------------

impl Database {
    /// 读取路由配置（单行，id=1）。不存在时返回默认值。
    pub fn autotier_get_config(&self) -> Result<AutotierRoutingConfigDto, AppError> {
        let conn = lock_conn!(self.conn);
        let row = conn
            .query_row(
                "SELECT mode, retention_days, raw_prompt_opt_in,
                        classifier_version, feature_version, policy_version, updated_at
                 FROM autotier_routing_config
                 WHERE id = 1",
                [],
                |row| {
                    Ok(AutotierRoutingConfigDto {
                        mode: row.get(0)?,
                        retention_days: row.get(1)?,
                        raw_prompt_opt_in: row.get(2)?,
                        classifier_version: row.get(3)?,
                        feature_version: row.get(4)?,
                        policy_version: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(row.unwrap_or_default())
    }

    /// 更新路由配置（幂等，单行 upsert）。
    pub fn autotier_set_config(&self, config: &AutotierRoutingConfigDto) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO autotier_routing_config (
                id, mode, retention_days, raw_prompt_opt_in,
                classifier_version, feature_version, policy_version, updated_at
            ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                mode = excluded.mode,
                retention_days = excluded.retention_days,
                raw_prompt_opt_in = excluded.raw_prompt_opt_in,
                classifier_version = excluded.classifier_version,
                feature_version = excluded.feature_version,
                policy_version = excluded.policy_version,
                updated_at = excluded.updated_at",
            params![
                config.mode,
                config.retention_days,
                config.raw_prompt_opt_in,
                config.classifier_version,
                config.feature_version,
                config.policy_version,
                config.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("autotier_set_config failed: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Provider Slot DAO
// ---------------------------------------------------------------------------

impl Database {
    /// 插入或更新 Provider Slot 映射（幂等，按 provider_id + slot 主键）。
    pub fn autotier_upsert_slot(&self, slot: &AutotierProviderSlotDto) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO autotier_provider_slots (
                provider_id, slot, model_id, capability_status,
                supports_tools, supports_streaming, supports_vision,
                context_limit, api_format,
                pricing_source, capability_source, verified_at,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(provider_id, slot) DO UPDATE SET
                model_id = excluded.model_id,
                capability_status = excluded.capability_status,
                supports_tools = excluded.supports_tools,
                supports_streaming = excluded.supports_streaming,
                supports_vision = excluded.supports_vision,
                context_limit = excluded.context_limit,
                api_format = excluded.api_format,
                pricing_source = excluded.pricing_source,
                capability_source = excluded.capability_source,
                verified_at = excluded.verified_at,
                updated_at = excluded.updated_at",
            params![
                slot.provider_id,
                slot.slot,
                slot.model_id,
                slot.capability_status,
                slot.supports_tools,
                slot.supports_streaming,
                slot.supports_vision,
                slot.context_limit,
                slot.api_format,
                slot.pricing_source,
                slot.capability_source,
                slot.verified_at,
                slot.created_at,
                slot.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("autotier_upsert_slot failed: {e}")))?;
        Ok(())
    }

    /// 查询某 Provider 的所有 Slot 映射。
    pub fn autotier_get_slots(
        &self,
        provider_id: &str,
    ) -> Result<Vec<AutotierProviderSlotDto>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, slot, model_id, capability_status,
                        supports_tools, supports_streaming, supports_vision,
                        context_limit, api_format,
                        pricing_source, capability_source, verified_at,
                        created_at, updated_at
                 FROM autotier_provider_slots
                 WHERE provider_id = ?1
                 ORDER BY slot",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([provider_id], |row| {
                Ok(AutotierProviderSlotDto {
                    provider_id: row.get(0)?,
                    slot: row.get(1)?,
                    model_id: row.get(2)?,
                    capability_status: row.get(3)?,
                    supports_tools: row.get(4)?,
                    supports_streaming: row.get(5)?,
                    supports_vision: row.get(6)?,
                    context_limit: row.get(7)?,
                    api_format: row.get(8)?,
                    pricing_source: row.get(9)?,
                    capability_source: row.get(10)?,
                    verified_at: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(rows)
    }

    /// 查询某 Provider 的特定 Slot。
    pub fn autotier_get_slot(
        &self,
        provider_id: &str,
        slot: &str,
    ) -> Result<Option<AutotierProviderSlotDto>, AppError> {
        let conn = lock_conn!(self.conn);
        let row = conn
            .query_row(
                "SELECT provider_id, slot, model_id, capability_status,
                        supports_tools, supports_streaming, supports_vision,
                        context_limit, api_format,
                        pricing_source, capability_source, verified_at,
                        created_at, updated_at
                 FROM autotier_provider_slots
                 WHERE provider_id = ?1 AND slot = ?2",
                params![provider_id, slot],
                |row| {
                    Ok(AutotierProviderSlotDto {
                        provider_id: row.get(0)?,
                        slot: row.get(1)?,
                        model_id: row.get(2)?,
                        capability_status: row.get(3)?,
                        supports_tools: row.get(4)?,
                        supports_streaming: row.get(5)?,
                        supports_vision: row.get(6)?,
                        context_limit: row.get(7)?,
                        api_format: row.get(8)?,
                        pricing_source: row.get(9)?,
                        capability_source: row.get(10)?,
                        verified_at: row.get(11)?,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(row)
    }

    /// 删除某 Provider 的所有 Slot（Provider 被删除时调用）。
    pub fn autotier_delete_slots_for_provider(&self, provider_id: &str) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        let count = conn
            .execute(
                "DELETE FROM autotier_provider_slots WHERE provider_id = ?1",
                [provider_id],
            )
            .map_err(|e| {
                AppError::Database(format!("autotier_delete_slots_for_provider failed: {e}"))
            })?;
        Ok(count as u64)
    }

    /// 检查某 Provider 是否已配置全部三个必需 Slot（Cheap/Mid/Strong）。
    pub fn autotier_has_required_slots(&self, provider_id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM autotier_provider_slots
                 WHERE provider_id = ?1 AND slot IN ('cheap', 'mid', 'strong')",
                [provider_id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count >= 3)
    }
}

// ---------------------------------------------------------------------------
// 测试辅助：在内存数据库上运行 DAO 测试
// ---------------------------------------------------------------------------
// Phase 7A decision list/detail queries
// ---------------------------------------------------------------------------

#[path = "autotier_decisions_query.rs"]
mod autotier_decisions_query;
pub use autotier_decisions_query::{
    AutotierDecisionDetail, AutotierDecisionListPage, AutotierDecisionQueryFilter,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 辅助：创建内存数据库并应用 schema（含 autotier 表）。
    fn test_db() -> Database {
        Database::memory().expect("failed to create memory db")
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

    // --- Decision DAO ---

    #[test]
    fn upsert_and_get_decision() {
        let db = test_db();
        let row = make_decision_row("d-upsert-1");
        db.autotier_upsert_decision(&row).unwrap();

        let fetched = db.autotier_get_decision("d-upsert-1").unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.decision_id, "d-upsert-1");
        assert_eq!(fetched.mode, "shadow");
        assert!(!fetched.is_complete);
        assert!(!fetched.autotier_mutated_request);
    }

    #[test]
    fn upsert_decision_is_idempotent() {
        let db = test_db();
        let row = make_decision_row("d-idem-1");
        db.autotier_upsert_decision(&row).unwrap();
        db.autotier_upsert_decision(&row).unwrap();

        assert_eq!(db.autotier_count_decisions().unwrap(), 1);
    }

    #[test]
    fn upsert_decision_preserves_labels() {
        let db = test_db();
        let row = make_decision_row("d-idem-label");
        db.autotier_upsert_decision(&row).unwrap();

        let label = AutotierDecisionLabelDto {
            decision_id: "d-idem-label".to_string(),
            label: "correct".to_string(),
            reason: None,
            note: None,
            created_at: 1_000,
            updated_at: 1_000,
        };
        db.autotier_upsert_label(&label).unwrap();
        assert_eq!(db.autotier_count_labels().unwrap(), 1);

        // 重复写入同一 decision 不应级联删除 label
        db.autotier_upsert_decision(&row).unwrap();
        assert_eq!(db.autotier_count_labels().unwrap(), 1);
    }

    #[test]
    fn upsert_decision_does_not_overwrite_finalized() {
        let db = test_db();
        let row = make_decision_row("d-idem-final");
        db.autotier_upsert_decision(&row).unwrap();
        db.autotier_finalize_decision(&FinalizeDecisionParams {
            decision_id: "d-idem-final",
            completed_at: 1_700_000_100,
            actual_outbound_model: Some("claude-sonnet-4-20250514"),
            actual_outbound_provider: Some("provider-a"),
            upstream_message_id: Some("msg-001"),
            usage_request_id: Some("session:msg-001"),
            actual_input_tokens: Some(100),
            actual_output_tokens: Some(50),
            actual_cache_read_tokens: Some(80),
            actual_cache_write_5m_tokens: Some(20),
            actual_cache_write_1h_tokens: Some(0),
            actual_cost_usd: Some("0.0015"),
            status_code: Some(200),
            outcome: Some("success"),
            retry_count: Some(0),
            fallback_count: Some(0),
            error_code: None,
        })
        .unwrap();

        // 初始重复写入不应把 finalized 字段覆盖回空
        db.autotier_upsert_decision(&row).unwrap();
        let fetched = db.autotier_get_decision("d-idem-final").unwrap().unwrap();
        assert_eq!(fetched.upstream_message_id, Some("msg-001".to_string()));
        assert_eq!(
            fetched.usage_request_id,
            Some("session:msg-001".to_string())
        );
        assert_eq!(fetched.actual_input_tokens, Some(100));
        assert_eq!(fetched.actual_cost_usd, Some("0.0015".to_string()));
        assert_eq!(fetched.status_code, Some(200));
        assert!(fetched.is_complete);
    }

    #[test]
    fn finalize_missing_decision_fails() {
        let db = test_db();
        let result = db.autotier_finalize_decision(&FinalizeDecisionParams {
            decision_id: "d-missing",
            completed_at: 1_700_000_100,
            ..Default::default()
        });
        assert!(result.is_err(), "finalize 不存在的 decision 必须报错");
    }

    #[test]
    fn finalize_without_usage_keeps_incomplete() {
        let db = test_db();
        let row = make_decision_row("d-no-usage");
        db.autotier_upsert_decision(&row).unwrap();

        db.autotier_finalize_decision(&FinalizeDecisionParams {
            decision_id: "d-no-usage",
            completed_at: 1_700_000_100,
            actual_outbound_model: Some("claude-sonnet-4-20250514"),
            actual_outbound_provider: Some("provider-a"),
            upstream_message_id: Some("msg-001"),
            usage_request_id: None,
            actual_input_tokens: None,
            actual_output_tokens: None,
            actual_cache_read_tokens: None,
            actual_cache_write_5m_tokens: None,
            actual_cache_write_1h_tokens: None,
            actual_cost_usd: None,
            status_code: Some(200),
            outcome: Some("success"),
            retry_count: Some(0),
            fallback_count: Some(0),
            error_code: None,
        })
        .unwrap();

        let fetched = db.autotier_get_decision("d-no-usage").unwrap().unwrap();
        assert!(
            !fetched.is_complete,
            "没有 usage_request_id/token 不应标记 complete"
        );
        assert_eq!(fetched.status_code, Some(200));
    }

    #[test]
    fn finalize_with_usage_becomes_complete() {
        let db = test_db();
        let row = make_decision_row("d-with-usage");
        db.autotier_upsert_decision(&row).unwrap();

        db.autotier_finalize_decision(&FinalizeDecisionParams {
            decision_id: "d-with-usage",
            completed_at: 1_700_000_100,
            actual_outbound_model: Some("claude-sonnet-4-20250514"),
            actual_outbound_provider: Some("provider-a"),
            upstream_message_id: Some("msg-001"),
            usage_request_id: Some("session:msg-001"),
            actual_input_tokens: None,
            actual_output_tokens: None,
            actual_cache_read_tokens: None,
            actual_cache_write_5m_tokens: None,
            actual_cache_write_1h_tokens: None,
            actual_cost_usd: None,
            status_code: Some(200),
            outcome: Some("success"),
            retry_count: Some(0),
            fallback_count: Some(0),
            error_code: None,
        })
        .unwrap();

        let fetched = db.autotier_get_decision("d-with-usage").unwrap().unwrap();
        assert!(fetched.is_complete, "有 usage_request_id 应标记 complete");
    }

    #[test]
    fn finalize_decision_sets_complete() {
        let db = test_db();
        let row = make_decision_row("d-fin-1");
        db.autotier_upsert_decision(&row).unwrap();

        db.autotier_finalize_decision(&FinalizeDecisionParams {
            decision_id: "d-fin-1",
            completed_at: 1_700_000_100,
            actual_outbound_model: Some("claude-sonnet-4-20250514"),
            actual_outbound_provider: Some("provider-a"),
            upstream_message_id: Some("msg-001"),
            usage_request_id: Some("session:msg-001"),
            actual_input_tokens: Some(100),
            actual_output_tokens: Some(50),
            actual_cache_read_tokens: Some(80),
            actual_cache_write_5m_tokens: Some(20),
            actual_cache_write_1h_tokens: Some(0),
            actual_cost_usd: Some("0.0015"),
            status_code: Some(200),
            outcome: Some("success"),
            retry_count: Some(0),
            fallback_count: Some(0),
            error_code: None,
        })
        .unwrap();

        let fetched = db.autotier_get_decision("d-fin-1").unwrap().unwrap();
        assert!(fetched.is_complete);
        assert_eq!(fetched.completed_at, Some(1_700_000_100));
        assert_eq!(fetched.upstream_message_id, Some("msg-001".to_string()));
        assert_eq!(
            fetched.usage_request_id,
            Some("session:msg-001".to_string())
        );
        assert_eq!(fetched.actual_input_tokens, Some(100));
        assert_eq!(fetched.actual_cost_usd, Some("0.0015".to_string()));
        assert_eq!(fetched.status_code, Some(200));
    }

    #[test]
    fn finalize_preserves_existing_fields_when_none() {
        let db = test_db();
        let mut row = make_decision_row("d-fin-2");
        row.actual_input_tokens = Some(999);
        db.autotier_upsert_decision(&row).unwrap();

        // Finalize with None for input_tokens → should keep 999
        db.autotier_finalize_decision(&FinalizeDecisionParams {
            decision_id: "d-fin-2",
            completed_at: 1_700_000_200,
            ..Default::default()
        })
        .unwrap();

        let fetched = db.autotier_get_decision("d-fin-2").unwrap().unwrap();
        assert_eq!(fetched.actual_input_tokens, Some(999));
    }

    #[test]
    fn list_decisions_by_time_range() {
        let db = test_db();
        for i in 0..5 {
            let mut row = make_decision_row(&format!("d-list-{i}"));
            row.created_at = 1_000 + i;
            db.autotier_upsert_decision(&row).unwrap();
        }

        let rows = db.autotier_list_decisions(1_000, 1_005, 10, 0).unwrap();
        assert_eq!(rows.len(), 5);
        // DESC order
        assert_eq!(rows[0].created_at, 1_004);
    }

    #[test]
    fn prune_decisions_by_retention() {
        let db = test_db();
        let now = chrono::Local::now().timestamp();
        // Old decision (40 days ago)
        let mut old = make_decision_row("d-prune-old");
        old.created_at = now - 40 * 86_400;
        db.autotier_upsert_decision(&old).unwrap();
        // Recent decision
        let mut recent = make_decision_row("d-prune-recent");
        recent.created_at = now - 1;
        db.autotier_upsert_decision(&recent).unwrap();

        let deleted = db.autotier_prune_decisions(30).unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(db.autotier_count_decisions().unwrap(), 1);
    }

    #[test]
    fn prune_zero_retention_clears_all() {
        let db = test_db();
        db.autotier_upsert_decision(&make_decision_row("d-prune-0-1"))
            .unwrap();
        db.autotier_upsert_decision(&make_decision_row("d-prune-0-2"))
            .unwrap();

        let deleted = db.autotier_prune_decisions(0).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(db.autotier_count_decisions().unwrap(), 0);
    }

    #[test]
    fn clear_all_decisions() {
        let db = test_db();
        db.autotier_upsert_decision(&make_decision_row("d-clear-1"))
            .unwrap();
        db.autotier_upsert_decision(&make_decision_row("d-clear-2"))
            .unwrap();

        db.autotier_clear_all_decisions().unwrap();
        assert_eq!(db.autotier_count_decisions().unwrap(), 0);
    }

    #[test]
    fn count_sessions() {
        let db = test_db();
        let mut d1 = make_decision_row("d-sess-1");
        d1.session_id_hash = "hash-a".to_string();
        let mut d2 = make_decision_row("d-sess-2");
        d2.session_id_hash = "hash-a".to_string();
        let mut d3 = make_decision_row("d-sess-3");
        d3.session_id_hash = "hash-b".to_string();
        db.autotier_upsert_decision(&d1).unwrap();
        db.autotier_upsert_decision(&d2).unwrap();
        db.autotier_upsert_decision(&d3).unwrap();

        assert_eq!(db.autotier_count_sessions().unwrap(), 2);
    }

    // --- Label DAO ---

    #[test]
    fn upsert_and_get_label() {
        let db = test_db();
        db.autotier_upsert_decision(&make_decision_row("d-lbl-1"))
            .unwrap();

        let label = AutotierDecisionLabelDto {
            decision_id: "d-lbl-1".to_string(),
            label: "correct".to_string(),
            reason: Some("simple_formatting".to_string()),
            note: Some("test note".to_string()),
            created_at: 1_000,
            updated_at: 1_000,
        };
        db.autotier_upsert_label(&label).unwrap();

        let fetched = db.autotier_get_label("d-lbl-1").unwrap().unwrap();
        assert_eq!(fetched.label, "correct");
        assert_eq!(fetched.reason, Some("simple_formatting".to_string()));
    }

    #[test]
    fn upsert_label_updates_existing() {
        let db = test_db();
        db.autotier_upsert_decision(&make_decision_row("d-lbl-2"))
            .unwrap();

        let label = AutotierDecisionLabelDto {
            decision_id: "d-lbl-2".to_string(),
            label: "correct".to_string(),
            reason: None,
            note: None,
            created_at: 1_000,
            updated_at: 1_000,
        };
        db.autotier_upsert_label(&label).unwrap();

        let updated = AutotierDecisionLabelDto {
            decision_id: "d-lbl-2".to_string(),
            label: "should_be_stronger".to_string(),
            reason: Some("architecture_reasoning".to_string()),
            note: None,
            created_at: 1_000,
            updated_at: 2_000,
        };
        db.autotier_upsert_label(&updated).unwrap();

        let fetched = db.autotier_get_label("d-lbl-2").unwrap().unwrap();
        assert_eq!(fetched.label, "should_be_stronger");
        assert_eq!(fetched.updated_at, 2_000);
        assert_eq!(db.autotier_count_labels().unwrap(), 1);
    }

    #[test]
    fn label_cascade_delete_with_decision() {
        let db = test_db();
        db.autotier_upsert_decision(&make_decision_row("d-cascade-1"))
            .unwrap();
        let label = AutotierDecisionLabelDto {
            decision_id: "d-cascade-1".to_string(),
            label: "correct".to_string(),
            reason: None,
            note: None,
            created_at: 1_000,
            updated_at: 1_000,
        };
        db.autotier_upsert_label(&label).unwrap();
        assert_eq!(db.autotier_count_labels().unwrap(), 1);

        // Delete the decision row directly → label should cascade.
        // lock_conn! uses `?`, so we need a Result-returning closure.
        let delete_result = || -> Result<(), AppError> {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "DELETE FROM autotier_routing_decisions WHERE decision_id = 'd-cascade-1'",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        };
        delete_result().unwrap();
        assert_eq!(db.autotier_count_labels().unwrap(), 0);
    }

    // --- Config DAO ---

    #[test]
    fn config_defaults_when_empty() {
        let db = test_db();
        let config = db.autotier_get_config().unwrap();
        assert_eq!(config.mode, "shadow");
        assert_eq!(config.retention_days, 30);
        assert!(!config.raw_prompt_opt_in);
    }

    #[test]
    fn set_and_get_config() {
        let db = test_db();
        let config = AutotierRoutingConfigDto {
            mode: "off".to_string(),
            retention_days: 7,
            raw_prompt_opt_in: false,
            classifier_version: "v0.2".to_string(),
            feature_version: "v0.2".to_string(),
            policy_version: "v0.2".to_string(),
            updated_at: 1_700_000_000,
        };
        db.autotier_set_config(&config).unwrap();

        let fetched = db.autotier_get_config().unwrap();
        assert_eq!(fetched.mode, "off");
        assert_eq!(fetched.retention_days, 7);
        assert_eq!(fetched.classifier_version, "v0.2");

        // Update
        let updated = AutotierRoutingConfigDto {
            mode: "shadow".to_string(),
            retention_days: 90,
            raw_prompt_opt_in: true,
            ..fetched
        };
        db.autotier_set_config(&updated).unwrap();
        let fetched2 = db.autotier_get_config().unwrap();
        assert_eq!(fetched2.mode, "shadow");
        assert_eq!(fetched2.retention_days, 90);
        assert!(fetched2.raw_prompt_opt_in);
    }

    // --- Slot DAO ---

    #[test]
    fn upsert_and_get_slot() {
        let db = test_db();
        let slot = AutotierProviderSlotDto {
            provider_id: "provider-a".to_string(),
            slot: "cheap".to_string(),
            model_id: "claude-haiku-3.5".to_string(),
            capability_status: "verified".to_string(),
            supports_tools: Some(true),
            supports_streaming: Some(true),
            supports_vision: Some(false),
            context_limit: Some(200_000),
            api_format: Some("anthropic".to_string()),
            pricing_source: Some("manual".to_string()),
            capability_source: Some("manual".to_string()),
            verified_at: Some(1_700_000_000),
            created_at: 1_700_000_000,
            updated_at: 1_700_000_000,
        };
        db.autotier_upsert_slot(&slot).unwrap();

        let fetched = db
            .autotier_get_slot("provider-a", "cheap")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.model_id, "claude-haiku-3.5");
        assert_eq!(fetched.supports_tools, Some(true));
    }

    #[test]
    fn upsert_slot_updates_existing() {
        let db = test_db();
        let slot = AutotierProviderSlotDto {
            provider_id: "provider-a".to_string(),
            slot: "mid".to_string(),
            model_id: "claude-sonnet-4".to_string(),
            capability_status: "unknown".to_string(),
            supports_tools: None,
            supports_streaming: None,
            supports_vision: None,
            context_limit: None,
            api_format: None,
            pricing_source: None,
            capability_source: None,
            verified_at: None,
            created_at: 1_000,
            updated_at: 1_000,
        };
        db.autotier_upsert_slot(&slot).unwrap();

        let updated = AutotierProviderSlotDto {
            model_id: "claude-sonnet-4-20250514".to_string(),
            capability_status: "verified".to_string(),
            supports_tools: Some(true),
            updated_at: 2_000,
            ..slot
        };
        db.autotier_upsert_slot(&updated).unwrap();

        let fetched = db.autotier_get_slot("provider-a", "mid").unwrap().unwrap();
        assert_eq!(fetched.model_id, "claude-sonnet-4-20250514");
        assert_eq!(fetched.capability_status, "verified");
        assert_eq!(fetched.updated_at, 2_000);
    }

    #[test]
    fn get_slots_for_provider() {
        let db = test_db();
        for slot_name in ["cheap", "mid", "strong"] {
            let slot = AutotierProviderSlotDto {
                provider_id: "provider-b".to_string(),
                slot: slot_name.to_string(),
                model_id: format!("model-{slot_name}"),
                capability_status: "verified".to_string(),
                supports_tools: Some(true),
                supports_streaming: Some(true),
                supports_vision: Some(false),
                context_limit: Some(200_000),
                api_format: Some("anthropic".to_string()),
                pricing_source: None,
                capability_source: None,
                verified_at: None,
                created_at: 1_000,
                updated_at: 1_000,
            };
            db.autotier_upsert_slot(&slot).unwrap();
        }

        let slots = db.autotier_get_slots("provider-b").unwrap();
        assert_eq!(slots.len(), 3);
    }

    #[test]
    fn delete_slots_for_provider() {
        let db = test_db();
        for slot_name in ["cheap", "mid", "strong"] {
            let slot = AutotierProviderSlotDto {
                provider_id: "provider-c".to_string(),
                slot: slot_name.to_string(),
                model_id: "m".to_string(),
                capability_status: "unknown".to_string(),
                supports_tools: None,
                supports_streaming: None,
                supports_vision: None,
                context_limit: None,
                api_format: None,
                pricing_source: None,
                capability_source: None,
                verified_at: None,
                created_at: 1_000,
                updated_at: 1_000,
            };
            db.autotier_upsert_slot(&slot).unwrap();
        }

        let deleted = db.autotier_delete_slots_for_provider("provider-c").unwrap();
        assert_eq!(deleted, 3);
        assert!(db.autotier_get_slots("provider-c").unwrap().is_empty());
    }

    #[test]
    fn has_required_slots() {
        let db = test_db();
        assert!(!db.autotier_has_required_slots("provider-d").unwrap());

        for slot_name in ["cheap", "mid", "strong"] {
            let slot = AutotierProviderSlotDto {
                provider_id: "provider-d".to_string(),
                slot: slot_name.to_string(),
                model_id: "m".to_string(),
                capability_status: "unknown".to_string(),
                supports_tools: None,
                supports_streaming: None,
                supports_vision: None,
                context_limit: None,
                api_format: None,
                pricing_source: None,
                capability_source: None,
                verified_at: None,
                created_at: 1_000,
                updated_at: 1_000,
            };
            db.autotier_upsert_slot(&slot).unwrap();
        }
        assert!(db.autotier_has_required_slots("provider-d").unwrap());
    }

    // --- Migration idempotency ---

    #[test]
    fn migration_to_v18_is_idempotent() {
        // 验证：迁移完成后 autotier 表存在且可写，版本号停在 18
        let conn = Connection::open_in_memory().unwrap();
        Database::create_tables_on_conn(&conn).unwrap();
        Database::apply_schema_migrations_on_conn(&conn).unwrap();
        assert_eq!(Database::get_user_version(&conn).unwrap(), 18);

        // 再次运行迁移不应报错
        Database::apply_schema_migrations_on_conn(&conn).unwrap();
        assert_eq!(Database::get_user_version(&conn).unwrap(), 18);

        // autotier 表存在
        assert!(Database::table_exists(&conn, "autotier_routing_decisions").unwrap());
        assert!(Database::table_exists(&conn, "autotier_routing_config").unwrap());
        assert!(Database::table_exists(&conn, "autotier_provider_slots").unwrap());
        assert!(Database::table_exists(&conn, "autotier_decision_labels").unwrap());
    }

    #[test]
    fn migration_preserves_base_tables() {
        // 验证：迁移不破坏基座表
        let conn = Connection::open_in_memory().unwrap();
        Database::create_tables_on_conn(&conn).unwrap();
        Database::apply_schema_migrations_on_conn(&conn).unwrap();

        // 基座表仍然存在
        assert!(Database::table_exists(&conn, "providers").unwrap());
        assert!(Database::table_exists(&conn, "proxy_request_logs").unwrap());
        assert!(Database::table_exists(&conn, "mcp_servers").unwrap());
    }
}

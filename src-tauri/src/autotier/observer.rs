//! AutoTier Shadow Observer — Phase 4
//!
//! 在代理请求链入口做薄层观测：
//! - 提取请求特征并运行 Shadow 决策
//! - 构造一条未完成的决策记录（`is_complete=false`）
//! - 由调用方 `tokio::spawn` 异步落库，失败不影响请求
//!
//! 本模块不做 I/O，不阻塞请求，所有字段构造都是纯内存计算。

use crate::app_config::AppType;
use crate::database::{AutotierDecisionRow, AutotierRoutingConfigDto};
use sha2::{Digest, Sha256};

use super::{
    extract_features, shadow_decide, DecisionInput, RoutingDecision, RoutingMode, SessionIdHash,
    FEATURE_VERSION,
};

/// 检查配置是否启用 Shadow 观测。
///
/// v0.1 仅 `off` 和 `shadow` 激活：配置非 `shadow` 时跳过全部逻辑。
pub fn is_shadow_enabled(config: &AutotierRoutingConfigDto) -> bool {
    config.mode == "shadow"
}

/// 配置读取成功且 mode=shadow 时才观测；读取失败则旁路（不启用 Shadow）。
pub fn shadow_config_for_observe<E: std::fmt::Display>(
    result: Result<AutotierRoutingConfigDto, E>,
) -> Option<AutotierRoutingConfigDto> {
    match result {
        Ok(config) if is_shadow_enabled(&config) => Some(config),
        Ok(_) => None,
        Err(e) => {
            log::warn!("[AutoTier] config read failed, skip shadow: {e}");
            None
        }
    }
}

/// 对 Session ID 做 SHA-256 hex 哈希。
///
/// Session ID 原文不进入决策持久化结构，只存哈希（PRD §9.1 隐私约束）。
pub fn hash_session_id(session_id: &str) -> SessionIdHash {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    SessionIdHash(format!("{:x}", hasher.finalize()))
}

/// Shadow 观测所需的请求元数据。
///
/// 从 `RequestContext` 提取的纯值副本，避免 observer 耦合 handler 内部结构。
#[derive(Debug, Clone)]
pub struct ShadowInput {
    pub decision_id: String,
    pub app_type: AppType,
    pub session_id: String,
    pub request_model: String,
    pub provider_id: String,
}

/// 构建 Shadow 观测数据库行。
///
/// 入口阶段只记录客户端请求组与候选建议。Baseline/Actual 必须等 Forwarder
/// 回填真实出站（Phase 4C），此处保持 `None`，不得把入口 `request_model` /
/// 初始 Provider 冒充为基线或实际出站。
///
/// Shadow 不变量（PRD §7.3 FR-DEC-003）在入口对 `None == None` 成立：
/// - `autotier_mutated_request = false`
/// - `actual_outbound_* == baseline_outbound_*`
///
/// 返回 `(row, decision)` 对：`row` 用于异步写入 DB，`decision` 用于内存断言。
pub fn build_shadow_row(
    input: &ShadowInput,
    body: &serde_json::Value,
    _config: &AutotierRoutingConfigDto,
) -> (AutotierDecisionRow, RoutingDecision) {
    let session_hash = hash_session_id(&input.session_id);
    let features = extract_features(body, input.app_type.clone(), &session_hash.0);

    let decision_input = DecisionInput {
        decision_id: super::DecisionId(input.decision_id.clone()),
        app_type: input.app_type.clone(),
        client_requested_model: input.request_model.clone(),
        initial_selected_provider: Some(input.provider_id.clone()),
        features: features.clone(),
        session_state: super::RoutingSessionState::default(),
        mode: RoutingMode::Shadow,
        feature_version: FEATURE_VERSION.to_string(),
    };

    let engine = shadow_decide(&decision_input, 0);

    let decision = RoutingDecision {
        decision_id: super::DecisionId(input.decision_id.clone()),
        session_id_hash: session_hash,
        upstream_message_id: super::UpstreamMessageId(None),
        usage_request_id: super::UsageRequestId(None),
        mode: RoutingMode::Shadow,
        app_type: input.app_type.clone(),
        client_request: super::ClientRequestFields {
            client_requested_model: input.request_model.clone(),
            initial_selected_provider: Some(input.provider_id.clone()),
        },
        baseline_outbound: super::BaselineOutboundFields {
            baseline_outbound_model: None,
            baseline_outbound_provider: None,
        },
        candidate: super::CandidateFields {
            recommended_slot: engine.recommended_slot,
            candidate_model: None,
            candidate_provider: None,
        },
        actual_outbound: super::ActualOutboundFields {
            actual_outbound_model: None,
            actual_outbound_provider: None,
        },
        autotier_mutated_request: false,
        complexity_score: engine.complexity_score,
        confidence: engine.confidence,
        reason_codes: engine.reason_codes.clone(),
        safe_to_execute: engine.safe_to_execute,
        unsafe_reasons: engine.unsafe_reasons.clone(),
        feature_version: FEATURE_VERSION.to_string(),
        classifier_version: engine.classifier_version.clone(),
        policy_version: engine.policy_version.clone(),
        is_complete: false,
    };

    let now = chrono::Utc::now().timestamp_millis();
    let row = AutotierDecisionRow {
        decision_id: decision.decision_id.0.clone(),
        created_at: now,
        completed_at: None,
        app_type: decision.app_type.as_str().to_string(),
        session_id_hash: decision.session_id_hash.0.clone(),
        mode: "shadow".to_string(),

        client_requested_model: decision.client_request.client_requested_model.clone(),
        initial_selected_provider: decision.client_request.initial_selected_provider.clone(),

        baseline_outbound_model: decision.baseline_outbound.baseline_outbound_model.clone(),
        baseline_outbound_provider: decision
            .baseline_outbound
            .baseline_outbound_provider
            .clone(),

        recommended_slot: decision
            .candidate
            .recommended_slot
            .map(|s| s.as_str().to_string()),
        candidate_model: None,
        candidate_provider: None,

        actual_outbound_model: decision.actual_outbound.actual_outbound_model.clone(),
        actual_outbound_provider: decision.actual_outbound.actual_outbound_provider.clone(),

        autotier_mutated_request: false,

        upstream_message_id: None,
        usage_request_id: None,

        complexity_score: Some(decision.complexity_score as f64),
        confidence: Some(decision.confidence as f64),
        reason_codes_json: serde_json::to_string(&decision.reason_codes)
            .unwrap_or_else(|_| "[]".into()),
        unsafe_reasons_json: serde_json::to_string(&decision.unsafe_reasons)
            .unwrap_or_else(|_| "[]".into()),
        safe_to_execute: decision.safe_to_execute,

        feature_json: serde_json::to_string(&features).unwrap_or_else(|_| "{}".into()),
        feature_version: decision.feature_version.clone(),
        classifier_version: decision.classifier_version.clone(),
        policy_version: decision.policy_version.clone(),

        actual_input_tokens: None,
        actual_output_tokens: None,
        actual_cache_read_tokens: None,
        actual_cache_write_5m_tokens: None,
        actual_cache_write_1h_tokens: None,
        actual_cost_usd: None,

        candidate_cost_low_usd: None,
        candidate_cost_base_usd: None,
        candidate_cost_high_usd: None,
        cost_assumptions_json: "[]".into(),

        status_code: None,
        outcome: None,
        retry_count: 0,
        fallback_count: 0,
        is_complete: false,
        error_code: None,
    };

    (row, decision)
}

// ===========================================================================
// 测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autotier::ModelSlot;
    use crate::database::AutotierRoutingConfigDto;
    use serde_json::json;

    fn short_input(model: &str, provider: &str) -> ShadowInput {
        ShadowInput {
            decision_id: uuid::Uuid::new_v4().to_string(),
            app_type: AppType::Claude,
            session_id: "sess-abc".to_string(),
            request_model: model.to_string(),
            provider_id: provider.to_string(),
        }
    }

    fn short_body(model: &str) -> serde_json::Value {
        json!({
            "model": model,
            "messages": [{"role": "user", "content": "hi"}]
        })
    }

    #[test]
    fn shadow_preserves_client_request_and_leaves_outbound_unset() {
        let input = short_input("claude-sonnet-4-20250514", "provider-a");
        let body = short_body("claude-sonnet-4-20250514");
        let config = AutotierRoutingConfigDto::default();
        let (row, _dec) = build_shadow_row(&input, &body, &config);

        assert_eq!(row.client_requested_model, "claude-sonnet-4-20250514");
        assert_eq!(row.initial_selected_provider.as_deref(), Some("provider-a"));
        assert_eq!(row.baseline_outbound_model, None);
        assert_eq!(row.actual_outbound_model, None);
        assert_eq!(row.baseline_outbound_provider, None);
        assert_eq!(row.actual_outbound_provider, None);
        assert_eq!(row.candidate_model, None);
        assert_eq!(row.candidate_provider, None);
        assert!(!row.autotier_mutated_request);
        assert!(!row.is_complete);
        assert!(!row.safe_to_execute);
    }

    #[test]
    fn shadow_decision_is_shadow_safe() {
        let input = short_input("claude-sonnet-4-20250514", "provider-a");
        let body = short_body("claude-sonnet-4-20250514");
        let config = AutotierRoutingConfigDto::default();
        let (_row, decision) = build_shadow_row(&input, &body, &config);

        assert!(decision.is_shadow_safe(), "shadow invariant must hold");
    }

    #[test]
    fn is_shadow_enabled_only_for_shadow_mode() {
        let mut config = AutotierRoutingConfigDto::default();
        assert!(is_shadow_enabled(&config));
        config.mode = "off".to_string();
        assert!(!is_shadow_enabled(&config));
        config.mode = "canary_live".to_string();
        assert!(!is_shadow_enabled(&config));
    }

    #[test]
    fn hash_session_id_is_deterministic() {
        let h1 = hash_session_id("sess-xyz");
        let h2 = hash_session_id("sess-xyz");
        assert_eq!(h1.0, h2.0);
        assert!(!h1.0.is_empty());
        assert_ne!(h1.0, "sess-xyz");
    }

    #[test]
    fn short_request_recommends_cheap() {
        let input = short_input("claude-sonnet-4-20250514", "provider-a");
        let body = short_body("claude-sonnet-4-20250514");
        let config = AutotierRoutingConfigDto::default();
        let (row, _) = build_shadow_row(&input, &body, &config);

        assert_eq!(
            row.recommended_slot.as_deref(),
            Some(ModelSlot::Cheap.as_str())
        );
    }

    #[test]
    fn feature_json_contains_no_raw_session() {
        let input = short_input("claude-sonnet-4-20250514", "provider-a");
        let body = short_body("claude-sonnet-4-20250514");
        let config = AutotierRoutingConfigDto::default();
        let (row, _) = build_shadow_row(&input, &body, &config);

        assert!(!row.feature_json.contains("sess-abc"));
        let parsed: serde_json::Value = serde_json::from_str(&row.feature_json).unwrap();
        assert!(parsed.get("original_model").is_some());
    }

    #[test]
    fn feature_json_contains_no_raw_prompt() {
        let canary = "CANARY_PROMPT_SECRET_4A_do_not_persist";
        let input = short_input("claude-sonnet-4-20250514", "provider-a");
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": canary}]
        });
        let config = AutotierRoutingConfigDto::default();
        let (row, _) = build_shadow_row(&input, &body, &config);

        assert!(
            !row.feature_json.contains(canary),
            "feature_json must not contain raw prompt"
        );
    }

    #[test]
    fn shadow_config_for_observe_fails_open_on_error() {
        let mut shadow = AutotierRoutingConfigDto::default();
        shadow.mode = "shadow".to_string();
        assert!(shadow_config_for_observe::<&str>(Ok(shadow.clone())).is_some());

        shadow.mode = "off".to_string();
        assert!(shadow_config_for_observe::<&str>(Ok(shadow)).is_none());

        let failed: Result<AutotierRoutingConfigDto, &str> = Err("db locked");
        assert!(shadow_config_for_observe(failed).is_none());
    }
}

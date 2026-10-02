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

use super::{
    cost::initial_cost_assumptions_json, extract_features, hash_session_id, shadow_decide,
    DecisionInput, RoutingDecision, RoutingMode, FEATURE_VERSION,
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
    pub session_state: super::RoutingSessionState,
}

/// 构建 Shadow 观测数据库行，兼容不需要读取 next state 的调用方。
pub fn build_shadow_row(
    input: &ShadowInput,
    body: &serde_json::Value,
    config: &AutotierRoutingConfigDto,
    secret: &[u8],
) -> (AutotierDecisionRow, RoutingDecision) {
    let (row, decision, _) = build_shadow_row_with_state(input, body, config, secret);
    (row, decision)
}

/// 构建 Shadow 行并返回本轮计算出的会话状态。
///
/// `next_state` 只供进程内会话存储使用，不写入数据库；数据库中的 feature_json
/// 只保留本轮决策前的有界历史窗口。
pub fn build_shadow_row_with_state(
    input: &ShadowInput,
    body: &serde_json::Value,
    _config: &AutotierRoutingConfigDto,
    secret: &[u8],
) -> (
    AutotierDecisionRow,
    RoutingDecision,
    super::RoutingSessionState,
) {
    let session_hash = hash_session_id(&input.session_id, secret);
    let mut features = extract_features(body, input.app_type.clone(), &session_hash.0);

    // TTL 净化：上一轮槽位与真实缓存命中都只在 prompt cache 生命周期内有效，
    // 过期后按新会话处理；复杂度窗口与请求计数不受 TTL 影响。
    // 决策引擎保持无时钟纯函数（PRD §12.3），时间判断收敛在 observer 层。
    let now = chrono::Utc::now().timestamp_millis();
    let mut session_state = input.session_state.clone();
    if session_state.fresh_last_slot(now).is_none() {
        session_state.last_recommended_slot = None;
    }
    if !session_state.has_fresh_cache_evidence(now) {
        session_state.last_cache_read_tokens = 0;
        session_state.last_cache_read_at = None;
    }

    // 把上一轮的有界状态带进本轮特征快照，便于 replay/导出解释会话趋势。
    features.recent_complexity_window = session_state.recent_complexity_scores.clone();

    let decision_input = DecisionInput {
        decision_id: super::DecisionId(input.decision_id.clone()),
        app_type: input.app_type.clone(),
        client_requested_model: input.request_model.clone(),
        initial_selected_provider: Some(input.provider_id.clone()),
        features: features.clone(),
        session_state,
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
        vision_fallback_applied: false,
        vision_describe_input_tokens: None,
        vision_describe_output_tokens: None,

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
        cost_assumptions_json: initial_cost_assumptions_json(body),

        status_code: None,
        outcome: None,
        retry_count: 0,
        fallback_count: 0,
        is_complete: false,
        error_code: None,
    };

    // 槽位推荐携带记录时刻返回给会话存储，供下一轮做 TTL 净化。
    let mut next_state = engine.next_state;
    if next_state.last_recommended_slot.is_some() {
        next_state.last_slot_recorded_at = Some(now);
    }

    (row, decision, next_state)
}

// ===========================================================================
// 测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autotier::hash_session_id;
    use crate::autotier::ModelSlot;
    use crate::database::AutotierRoutingConfigDto;
    use serde_json::json;

    const TEST_SECRET: [u8; 32] = [0x11; 32];

    fn short_input(model: &str, provider: &str) -> ShadowInput {
        ShadowInput {
            decision_id: uuid::Uuid::new_v4().to_string(),
            app_type: AppType::Claude,
            session_id: "sess-abc".to_string(),
            request_model: model.to_string(),
            provider_id: provider.to_string(),
            session_state: super::super::RoutingSessionState::default(),
        }
    }

    fn short_body(model: &str) -> serde_json::Value {
        json!({
            "model": model,
            "messages": [{"role": "user", "content": "hi"}]
        })
    }

    /// 端到端：store → observer → usage 反馈 → observer 的完整闭环。
    ///
    /// 覆盖 8b42e196/73916af1 引入的全部新语义，模拟代理处理同一会话的连续
    /// 请求：请求 1 简单（Cheap）；响应真实命中缓存并写回；请求 2 仍简单但因
    /// 真实缓存证据保持 Strong；之后每轮无缓存反馈，连续保护上限到期后降回
    /// Cheap；期间复杂度窗口与请求计数持续累积。
    #[test]
    fn shadow_session_loop_tracks_feedback_protection_and_latch_break() {
        let store = crate::autotier::RoutingSessionStore::default();
        let config = AutotierRoutingConfigDto::default();
        let key = crate::autotier::RoutingSessionKey::new(
            "claude",
            "provider-a",
            hash_session_id("sess-chain", &TEST_SECRET),
        );

        let simple = || {
            json!({
                "model": "claude-sonnet-4-20250514",
                "messages": [{"role": "user", "content": "hi"}]
            })
        };
        let run_request =
            |session_id: &str| -> (AutotierDecisionRow, crate::autotier::RoutingDecision) {
                store.update_with(key.clone(), |state| {
                    let (row, decision, next) = build_shadow_row_with_state(
                        &ShadowInput {
                            decision_id: uuid::Uuid::new_v4().to_string(),
                            app_type: AppType::Claude,
                            session_id: session_id.to_string(),
                            request_model: "claude-sonnet-4-20250514".to_string(),
                            provider_id: "provider-a".to_string(),
                            session_state: state.clone(),
                        },
                        &simple(),
                        &config,
                        &TEST_SECRET,
                    );
                    (next, (row, decision))
                })
            };

        // 轮 1：无状态 → 按阈值 Cheap，被推荐的槽位落盘为 Strong 以模拟
        // “首轮复杂”的实际场景？不——断言基线：简单请求就是 Cheap。
        let (row1, _) = run_request("sess-chain");
        assert_eq!(row1.recommended_slot.as_deref(), Some("cheap"));
        assert!(!row1.autotier_mutated_request);
        assert!(!row1.safe_to_execute);
        assert_eq!(row1.baseline_outbound_model, None);
        assert_eq!(row1.actual_outbound_model, None);

        // 人为把上一轮推荐提升为 Strong（模拟“首轮确实复杂”的真实结果），
        // 再写回一条真实 cache 命中反馈，如 Usage Finalize 所为。
        let now = chrono::Utc::now().timestamp_millis();
        assert!(store.update_existing_with(&key, |state| {
            let mut next = state.clone();
            next.last_recommended_slot = Some(ModelSlot::Strong);
            next.last_slot_recorded_at = Some(now);
            next.last_cache_read_tokens = 12_000;
            next.last_cache_read_at = Some(now);
            next
        }));

        // 轮 2～6：请求仍简单，但真实缓存证据 + 上一轮 Strong 使其被保护。
        for round in 2..=6 {
            let (row, _) = run_request("sess-chain");
            assert_eq!(
                row.recommended_slot.as_deref(),
                Some("strong"),
                "第 {round} 轮应被缓存保护保持 Strong"
            );
            assert_eq!(
                store.get(&key).unwrap().consecutive_cache_protections,
                (round - 1) as u32
            );
        }

        // 轮 7：连续保护已达 5 轮上限 → 强制按阈值重新评估，回落 Cheap。
        let (row7, _) = run_request("sess-chain");
        assert_eq!(row7.recommended_slot.as_deref(), Some("cheap"));
        assert_eq!(store.get(&key).unwrap().consecutive_cache_protections, 0);

        // 复杂度窗口与请求计数在 7 轮中持续累积。
        let state = store.get(&key).unwrap();
        assert_eq!(state.session_request_count, 7);
        assert_eq!(state.recent_complexity_scores.len(), 7);
    }

    /// 端到端：过期会话与切换 Provider 后保护不再生效，且复杂度窗口保留。
    #[test]
    fn shadow_session_loop_ttl_and_provider_isolation() {
        let store = crate::autotier::RoutingSessionStore::default();
        let config = AutotierRoutingConfigDto::default();
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "x".repeat(4000), "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": "hi"}]
        });

        let stale_at =
            chrono::Utc::now().timestamp_millis() - crate::autotier::SLOT_PROTECTION_TTL_MS - 1;
        let prime = |store: &crate::autotier::RoutingSessionStore,
                     key: &crate::autotier::RoutingSessionKey| {
            store.update_with(key.clone(), |_| {
                let mut next = crate::autotier::RoutingSessionState::default();
                next.session_request_count = 3;
                next.recent_complexity_scores = vec![0.3, 0.4, 0.5];
                next.last_recommended_slot = Some(ModelSlot::Strong);
                next.last_slot_recorded_at = Some(stale_at);
                next.last_cache_read_tokens = 9_000;
                next.last_cache_read_at = Some(stale_at);
                (next, ())
            });
        };
        let run = |store: &crate::autotier::RoutingSessionStore,
                   key: &crate::autotier::RoutingSessionKey| {
            store.update_with(key.clone(), |state| {
                let (row, _, next) = build_shadow_row_with_state(
                    &ShadowInput {
                        decision_id: uuid::Uuid::new_v4().to_string(),
                        app_type: AppType::Claude,
                        session_id: "sess-old".to_string(),
                        request_model: "claude-sonnet-4-20250514".to_string(),
                        provider_id: "provider-a".to_string(),
                        session_state: state.clone(),
                    },
                    &body,
                    &config,
                    &TEST_SECRET,
                );
                (next, row)
            })
        };

        // 场景 A：同一 key 但证据过期 → 不保护（即使 body 声明写缓存）。
        let key_a = crate::autotier::RoutingSessionKey::new(
            "claude",
            "provider-a",
            hash_session_id("sess-old", &TEST_SECRET),
        );
        prime(&store, &key_a);
        let row_a = run(&store, &key_a);
        assert_ne!(row_a.recommended_slot.as_deref(), Some("strong"));
        let state_a = store.get(&key_a).unwrap();
        // 复杂度窗口保留并累积，但过期槽位与缓存证据已清零。
        assert_eq!(state_a.recent_complexity_scores.len(), 4);
        assert_eq!(state_a.last_cache_read_tokens, 0);

        // 场景 B：Provider 切换 → 新 key 从默认状态开始。
        let key_b = crate::autotier::RoutingSessionKey::new(
            "claude",
            "provider-b",
            hash_session_id("sess-old", &TEST_SECRET),
        );
        assert_eq!(store.get(&key_b), None);
        let row_b = run(&store, &key_b);
        assert_ne!(row_b.recommended_slot.as_deref(), Some("strong"));
    }

    /// 端到端：生成 UUID 的匿名会话不建 store 条目，反馈写回静默跳过。
    #[test]
    fn shadow_session_loop_generated_session_ids_are_stateless() {
        let store = crate::autotier::RoutingSessionStore::default();
        let config = AutotierRoutingConfigDto::default();
        let body = short_body("claude-sonnet-4-20250514");

        for i in 0..3 {
            // 每轮用不同 UUID 模拟匿名会话；生产路径在
            // session_client_provided=false 时直接走此无状态分支。
            let session_id = format!("generated-uuid-{i}");
            let (row, _, _) = build_shadow_row_with_state(
                &ShadowInput {
                    decision_id: uuid::Uuid::new_v4().to_string(),
                    app_type: AppType::Claude,
                    session_id,
                    request_model: "claude-sonnet-4-20250514".to_string(),
                    provider_id: "provider-a".to_string(),
                    session_state: crate::autotier::RoutingSessionState::default(),
                },
                &body,
                &config,
                &TEST_SECRET,
            );
            assert_eq!(row.recommended_slot.as_deref(), Some("cheap"));
        }
        assert!(store.is_empty());

        let ghost = crate::autotier::RoutingSessionKey::new(
            "claude",
            "provider-a",
            hash_session_id("generated-uuid-0", &TEST_SECRET),
        );
        assert!(!store.update_existing_with(&ghost, |s| s.clone()));
        assert!(store.is_empty());
    }

    #[test]
    fn shadow_preserves_client_request_and_leaves_outbound_unset() {
        let input = short_input("claude-sonnet-4-20250514", "provider-a");
        let body = short_body("claude-sonnet-4-20250514");
        let config = AutotierRoutingConfigDto::default();
        let (row, _dec) = build_shadow_row(&input, &body, &config, &TEST_SECRET);

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
        let (_row, decision) = build_shadow_row(&input, &body, &config, &TEST_SECRET);

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
    fn hash_session_id_uses_hmac_not_raw_session() {
        let h1 = hash_session_id("sess-xyz", &TEST_SECRET);
        let h2 = hash_session_id("sess-xyz", &TEST_SECRET);
        assert_eq!(h1.0, h2.0);
        assert!(!h1.0.is_empty());
        assert_ne!(h1.0, "sess-xyz");
        assert!(!h1.0.contains("sess-xyz"));
    }

    #[test]
    fn short_request_recommends_cheap() {
        let input = short_input("claude-sonnet-4-20250514", "provider-a");
        let body = short_body("claude-sonnet-4-20250514");
        let config = AutotierRoutingConfigDto::default();
        let (row, _) = build_shadow_row(&input, &body, &config, &TEST_SECRET);

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
        let (row, _) = build_shadow_row(&input, &body, &config, &TEST_SECRET);

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
        let (row, _) = build_shadow_row(&input, &body, &config, &TEST_SECRET);

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

    #[test]
    fn stale_slot_is_purged_by_ttl_and_cache_protection_does_not_apply() {
        let mut input = short_input("claude-sonnet-4-20250514", "provider-a");
        // 上一轮推荐 Strong，但记录时刻已超出 prompt cache 生命周期：
        // 缓存连续性证据失效，本轮应恢复按阈值推荐，而不是保持 Strong。
        input.session_state = super::super::RoutingSessionState {
            recent_complexity_scores: vec![0.1],
            session_request_count: 1,
            last_recommended_slot: Some(ModelSlot::Strong),
            last_slot_recorded_at: Some(
                chrono::Utc::now().timestamp_millis() - crate::autotier::SLOT_PROTECTION_TTL_MS - 1,
            ),
            last_cache_read_tokens: 9_000,
            last_cache_read_at: Some(
                chrono::Utc::now().timestamp_millis() - crate::autotier::SLOT_PROTECTION_TTL_MS - 1,
            ),
            consecutive_cache_protections: 0,
        };
        // body 带 cache_control 块：若无 TTL 净化，缓存保护会错误保持 Strong。
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "x".repeat(4000), "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": "hi"}]
        });
        let config = AutotierRoutingConfigDto::default();
        let (_row, decision, next_state) =
            build_shadow_row_with_state(&input, &body, &config, &TEST_SECRET);

        assert_ne!(
            decision.candidate.recommended_slot,
            Some(ModelSlot::Strong),
            "过期槽位不得继续触发缓存保护"
        );
        // 推荐仍然记录新的时间戳，复杂度窗口继续累积；过期缓存证据已清零。
        assert!(next_state.last_slot_recorded_at.is_some());
        assert_eq!(next_state.last_cache_read_tokens, 0);
        assert_eq!(next_state.recent_complexity_scores.len(), 2);
        assert_eq!(next_state.session_request_count, 2);
    }

    #[test]
    fn shadow_uses_session_state_without_touching_outbound_fields() {
        let mut input = short_input("claude-sonnet-4-20250514", "provider-a");
        input.session_state = super::super::RoutingSessionState {
            recent_complexity_scores: vec![0.2, 0.35],
            session_request_count: 4,
            last_recommended_slot: Some(ModelSlot::Mid),
            last_slot_recorded_at: Some(chrono::Utc::now().timestamp_millis()),
            last_cache_read_tokens: 0,
            last_cache_read_at: None,
            consecutive_cache_protections: 0,
        };
        let body = short_body("claude-sonnet-4-20250514");
        let config = AutotierRoutingConfigDto::default();
        let (row, _decision, next_state) =
            build_shadow_row_with_state(&input, &body, &config, &TEST_SECRET);
        let feature_doc: serde_json::Value = serde_json::from_str(&row.feature_json).unwrap();
        assert_eq!(
            feature_doc["recent_complexity_window"],
            serde_json::json!([0.2, 0.35])
        );
        assert_eq!(next_state.session_request_count, 5);
        assert_eq!(next_state.last_recommended_slot, Some(ModelSlot::Cheap));
        assert_eq!(row.baseline_outbound_model, None);
        assert_eq!(row.actual_outbound_model, None);
        assert!(!row.autotier_mutated_request);
    }

    #[test]
    fn shadow_session_state_preserves_cached_higher_slot() {
        let mut input = short_input("claude-sonnet-4-20250514", "provider-a");
        input.session_state.last_recommended_slot = Some(ModelSlot::Strong);
        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{
                "type": "text",
                "text": "cached context",
                "cache_control": {"type": "ephemeral"}
            }],
            "messages": [{"role": "user", "content": "hi"}]
        });
        let config = AutotierRoutingConfigDto::default();
        let (row, _decision, next_state) =
            build_shadow_row_with_state(&input, &body, &config, &TEST_SECRET);
        assert_eq!(
            row.recommended_slot.as_deref(),
            Some(ModelSlot::Strong.as_str())
        );
        assert_eq!(next_state.last_recommended_slot, Some(ModelSlot::Strong));
    }

    #[test]
    fn cost_assumptions_record_cache_write_ttl_from_body() {
        let input = short_input("claude-sonnet-4-20250514", "provider-a");
        let config = AutotierRoutingConfigDto::default();

        let (unknown, _) = build_shadow_row(
            &input,
            &short_body("claude-sonnet-4-20250514"),
            &config,
            &TEST_SECRET,
        );
        let unknown_doc: serde_json::Value =
            serde_json::from_str(&unknown.cost_assumptions_json).unwrap();
        assert_eq!(unknown_doc["cache_write_ttl"], "unknown");
        assert_eq!(
            unknown_doc["capability_table_version"],
            crate::autotier::CAPABILITY_TABLE_VERSION
        );
        assert_eq!(
            unknown_doc["cost_model_version"],
            crate::autotier::COST_MODEL_VERSION
        );
        assert_eq!(
            unknown_doc["cache_stats_version"],
            crate::autotier::CACHE_STATS_VERSION
        );

        let body_5m = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": "hi"}]
        });
        let (row_5m, _) = build_shadow_row(&input, &body_5m, &config, &TEST_SECRET);
        let doc_5m: serde_json::Value =
            serde_json::from_str(&row_5m.cost_assumptions_json).unwrap();
        assert_eq!(doc_5m["cache_write_ttl"], "5m");

        let body_1h = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral", "ttl": "1h"}}],
            "messages": [{"role": "user", "content": "hi"}]
        });
        let (row_1h, _) = build_shadow_row(&input, &body_1h, &config, &TEST_SECRET);
        let doc_1h: serde_json::Value =
            serde_json::from_str(&row_1h.cost_assumptions_json).unwrap();
        assert_eq!(doc_1h["cache_write_ttl"], "1h");
    }
}

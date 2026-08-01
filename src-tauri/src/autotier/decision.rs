//! Decision Engine 契约 — DecisionInput、DecisionResult、ReasonCode、
//! UnsafeReason、RoutingSessionState（PRD §12）//!
//! 所有类型为纯数据定义或纯函数，不含 I/O、数据库或网络操作。
//! Clock 通过参数注入，相同版本 + 相同输入 = 相同输出（确定性）。

use serde::{Deserialize, Serialize};

use super::features::RoutingFeatures;
use super::{AgentType, DecisionId, ModelSlot, RoutingMode};

// ---------------------------------------------------------------------------
// ReasonCode（PRD §12.4，19 个稳定枚举）
// ---------------------------------------------------------------------------

/// 决策理由稳定枚举。
///
/// UI 不解析自由文本决定逻辑；所有决策理由必须映射到这些稳定 Code。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReasonCode {
    /// 用户消息很短。
    ShortUserRequest,
    /// 约束条件少。
    LowConstraintCount,
    /// 无活跃 Tool Loop。
    NoActiveToolLoop,
    /// 后台元数据请求。
    BackgroundMetadata,
    /// 客户端显式指定小模型。
    ExplicitSmallModel,
    /// 长上下文。
    LongContext,
    /// 多文件信号。
    MultiFileSignal,
    /// 存在 Tool Error。
    ToolErrorPresent,
    /// 约束条件多。
    HighConstraintCount,
    /// 推理信号。
    ReasoningSignal,
    /// 架构级信号。
    ArchitectureSignal,
    /// 多模态输入。
    MultimodalInput,
    /// 近期复杂度上升趋势。
    RecentComplexityRising,
    /// 缓存保护（换模型会破坏缓存命中）。
    CacheProtection,
    /// 模型能力未知。
    UnknownModelCapability,
    /// Provider Slot 不可用。
    ProviderSlotUnavailable,
    /// 用户强制 Slot（调试覆盖）。
    UserForcedSlot,
    /// 用户绕过。
    UserBypass,
    /// 分类器异常。
    ClassifierError,
}

// ---------------------------------------------------------------------------
// UnsafeReason — safe_to_execute 为 false 的原因
// ---------------------------------------------------------------------------

/// 阻止候选决策安全执行的原因（PRD §7.2 FR-MODE-003 + §12.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UnsafeReason {
    /// 分类器异常。
    ClassifierError,
    /// 配置缺失。
    ConfigMissing,
    /// Slot 无效。
    SlotInvalid,
    /// Provider 无候选模型。
    ProviderNoCandidate,
    /// 能力未知。
    CapabilityUnknown,
    /// Cost 模型不完整且策略要求 Cost Gate。
    CostModelIncomplete,
    /// Policy 版本不兼容。
    PolicyVersionIncompatible,
    /// Request Body 无法安全解析。
    RequestBodyUnparseable,
    /// 模型不支持 Tool Use。
    ToolUseNotSupported,
    /// 价格数据缺失。
    PriceMissing,
    /// 存在 Tool Error，降档风险高。
    ToolErrorPresent,
    /// 上下文超长，降档可能截断。
    LongContextExceeded,
}

// ---------------------------------------------------------------------------
// RoutingSessionState — 会话级运行状态（PRD §12.1/§12.3）
// ---------------------------------------------------------------------------

/// 会话级运行状态，由决策引擎维护并返回 `next_state`。
///
/// 运行状态不直接持久化为完整 Session（ADR-004）；
/// 决策日志与运行状态分离。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RoutingSessionState {
    /// 最近复杂度评分窗口（滑动窗口，旧值在前）。
    pub recent_complexity_scores: Vec<f32>,
    /// 当前 Session 的请求计数。
    pub session_request_count: u32,
    /// 上一次推荐的 Slot（用于 Session Stickiness / Cache Protection）。
    pub last_recommended_slot: Option<ModelSlot>,
}

impl RoutingSessionState {
    /// 纯函数：返回追加了新复杂度评分的新状态，不修改 `self`。
    ///
    /// `max_window` 控制滑动窗口大小，超出时丢弃最旧值。
    pub fn with_complexity_score(&self, score: f32, max_window: usize) -> Self {
        let mut scores = self.recent_complexity_scores.clone();
        scores.push(score);
        if scores.len() > max_window {
            scores.remove(0);
        }
        Self {
            recent_complexity_scores: scores,
            session_request_count: self.session_request_count + 1,
            last_recommended_slot: self.last_recommended_slot,
        }
    }

    /// 判断近期复杂度是否呈上升趋势（用于 `RECENT_COMPLEXITY_RISING`）。
    ///
    /// 需要至少 3 个数据点；比较后一半均值与前一半均值。
    pub fn is_complexity_rising(&self) -> bool {
        let n = self.recent_complexity_scores.len();
        if n < 3 {
            return false;
        }
        let mid = n / 2;
        let first_half: f32 =
            self.recent_complexity_scores[..mid].iter().sum::<f32>() / mid as f32;
        let second_half: f32 =
            self.recent_complexity_scores[mid..].iter().sum::<f32>() / (n - mid) as f32;
        second_half > first_half
    }
}

// ---------------------------------------------------------------------------
// DecisionInput（PRD §12.1）
// ---------------------------------------------------------------------------

/// 决策引擎输入。
///
/// 使用 `decision_id` + `client_requested_model`（§11.0 四组字段）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionInput {
    /// AutoTier 请求级内部主键（§11.0）。
    pub decision_id: DecisionId,
    /// 应用/Agent 类型（Agent-agnostic）。
    pub app_type: AgentType,
    /// 客户端请求的原始模型（§11.0 客户端请求组）。
    pub client_requested_model: String,
    /// Provider Router 首次选中的 Provider（§11.0 客户端请求组）。
    pub initial_selected_provider: Option<String>,
    /// 隐私安全派生特征。
    pub features: RoutingFeatures,
    /// 会话级运行状态。
    pub session_state: RoutingSessionState,
    /// 当前路由模式。
    pub mode: RoutingMode,
    /// 特征提取器版本。
    pub feature_version: String,
}

// ---------------------------------------------------------------------------
// DecisionResult（PRD §12.2）
// ---------------------------------------------------------------------------

/// 决策引擎输出。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResult {
    /// 推荐的逻辑槽位。
    pub recommended_slot: Option<ModelSlot>,
    /// 复杂度评分（0.0–1.0）。
    pub complexity_score: f32,
    /// 置信度（0.0–1.0）。
    pub confidence: f32,
    /// 决策理由（稳定枚举）。
    pub reason_codes: Vec<ReasonCode>,
    /// 是否可安全执行（v0.1 Shadow 默认 false）。
    pub safe_to_execute: bool,
    /// 不可安全执行的原因。
    pub unsafe_reasons: Vec<UnsafeReason>,
    /// 下一会话状态（纯函数：不修改输入状态）。
    pub next_state: RoutingSessionState,
    /// 分类器版本。
    pub classifier_version: String,
    /// 策略版本。
    pub policy_version: String,
}

impl DecisionResult {
    /// 构造 v0.1 Shadow 默认结果：`safe_to_execute` 始终为 false。
    pub fn shadow_noop(input: &DecisionInput) -> Self {
        Self {
            recommended_slot: None,
            complexity_score: 0.0,
            confidence: 0.0,
            reason_codes: vec![],
            safe_to_execute: false,
            unsafe_reasons: vec![UnsafeReason::CapabilityUnknown],
            next_state: input.session_state.clone(),
            classifier_version: SHADOW_CLASSIFIER_VERSION.to_string(),
            policy_version: SHADOW_POLICY_VERSION.to_string(),
        }
    }
}

/// Shadow stub 分类器版本。
pub const SHADOW_CLASSIFIER_VERSION: &str = "shadow-stub-v0.1";

/// Shadow stub 策略版本。
pub const SHADOW_POLICY_VERSION: &str = "shadow-stub-v0.1";

// ---------------------------------------------------------------------------
// shadow_decide — 纯函数 stub（Phase 3 将替换为真实分类器）
// ---------------------------------------------------------------------------

/// Shadow 决策 stub：纯函数，确定性，Clock 通过参数注入。
///
/// Phase 3 将替换为真实分类器；此 stub 演示类型契约和确定性约束：
/// - 相同版本 + 相同输入 = 相同输出
/// - v0.1 Shadow `safe_to_execute` 始终为 false
/// - `next_state` 不修改输入 `session_state`
///
/// # Arguments
/// * `input` - 决策输入（不可变引用）
/// * `_clock_ms` - 时钟参数（Unix 毫秒），Phase 3 用于时间衰减/冷却逻辑
pub fn shadow_decide(input: &DecisionInput, _clock_ms: u64) -> DecisionResult {
    let f = &input.features;
    let mut reasons = Vec::new();
    let mut score: f32 = 0.0;

    // --- 简单信号 → 复杂度评分 ---
    if f.user_message_weighted_length < 50 {
        reasons.push(ReasonCode::ShortUserRequest);
    } else {
        score += 0.15;
    }

    if f.constraint_count <= 1 {
        reasons.push(ReasonCode::LowConstraintCount);
    } else if f.constraint_count >= 5 {
        reasons.push(ReasonCode::HighConstraintCount);
        score += 0.2;
    }

    if f.tool_result_count == 0 && f.tool_definition_count == 0 {
        reasons.push(ReasonCode::NoActiveToolLoop);
    }

    if f.has_error_tool_result {
        reasons.push(ReasonCode::ToolErrorPresent);
        score += 0.3;
    }

    if f.code_structure_score > 0.6 {
        reasons.push(ReasonCode::ArchitectureSignal);
        score += 0.2;
    }

    if f.has_image_or_file {
        reasons.push(ReasonCode::MultimodalInput);
        score += 0.1;
    }

    if matches!(
        f.context_token_bucket,
        super::features::TokenBucket::Under128k | super::features::TokenBucket::Over128k
    ) {
        reasons.push(ReasonCode::LongContext);
        score += 0.15;
    }

    if input.session_state.is_complexity_rising() {
        reasons.push(ReasonCode::RecentComplexityRising);
        score += 0.1;
    }

    if f.cache_read_tokens > 0 || f.cache_write_tokens > 0 {
        reasons.push(ReasonCode::CacheProtection);
    }

    // --- 推荐槽位（stub 规则） ---
    let recommended = if score < 0.2 {
        Some(ModelSlot::Cheap)
    } else if score < 0.4 {
        Some(ModelSlot::Mid)
    } else {
        Some(ModelSlot::Strong)
    };

    // --- next_state: 纯函数，不修改输入 ---
    let next_state = input
        .session_state
        .with_complexity_score(score, 10);

    DecisionResult {
        recommended_slot: recommended,
        complexity_score: score,
        confidence: 0.5, // stub: 固定置信度
        reason_codes: reasons,
        // v0.1 Shadow: 始终 false（PRD §12.5）
        safe_to_execute: false,
        unsafe_reasons: vec![],
        next_state,
        classifier_version: SHADOW_CLASSIFIER_VERSION.to_string(),
        policy_version: SHADOW_POLICY_VERSION.to_string(),
    }
}

// ===========================================================================
// 测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::autotier::features::{CountBucket, RoutingFeatures, TokenBucket};

    // --- ReasonCode ---

    #[test]
    fn reason_code_has_19_variants() {
        // PRD §12.4: 确认全部 19 个变体存在且序列化正确
        let codes = [
            ReasonCode::ShortUserRequest,
            ReasonCode::LowConstraintCount,
            ReasonCode::NoActiveToolLoop,
            ReasonCode::BackgroundMetadata,
            ReasonCode::ExplicitSmallModel,
            ReasonCode::LongContext,
            ReasonCode::MultiFileSignal,
            ReasonCode::ToolErrorPresent,
            ReasonCode::HighConstraintCount,
            ReasonCode::ReasoningSignal,
            ReasonCode::ArchitectureSignal,
            ReasonCode::MultimodalInput,
            ReasonCode::RecentComplexityRising,
            ReasonCode::CacheProtection,
            ReasonCode::UnknownModelCapability,
            ReasonCode::ProviderSlotUnavailable,
            ReasonCode::UserForcedSlot,
            ReasonCode::UserBypass,
            ReasonCode::ClassifierError,
        ];
        assert_eq!(codes.len(), 19, "PRD §12.4 requires exactly 19 ReasonCodes");

        // 验证序列化为 SCREAMING_SNAKE_CASE
        let json = serde_json::to_string(&ReasonCode::ShortUserRequest).unwrap();
        assert_eq!(json, "\"SHORT_USER_REQUEST\"");

        let json = serde_json::to_string(&ReasonCode::ClassifierError).unwrap();
        assert_eq!(json, "\"CLASSIFIER_ERROR\"");
    }

    #[test]
    fn reason_code_serde_roundtrip() {
        for code in [
            ReasonCode::ShortUserRequest,
            ReasonCode::HighConstraintCount,
            ReasonCode::RecentComplexityRising,
            ReasonCode::CacheProtection,
            ReasonCode::UnknownModelCapability,
        ] {
            let json = serde_json::to_string(&code).unwrap();
            let back: ReasonCode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, code);
        }
    }

    // --- UnsafeReason ---

    #[test]
    fn unsafe_reason_serde() {
        let json = serde_json::to_string(&UnsafeReason::ClassifierError).unwrap();
        assert_eq!(json, "\"CLASSIFIER_ERROR\"");

        let json = serde_json::to_string(&UnsafeReason::CostModelIncomplete).unwrap();
        assert_eq!(json, "\"COST_MODEL_INCOMPLETE\"");
    }

    // --- RoutingSessionState ---

    #[test]
    fn session_state_default() {
        let s = RoutingSessionState::default();
        assert!(s.recent_complexity_scores.is_empty());
        assert_eq!(s.session_request_count, 0);
        assert!(s.last_recommended_slot.is_none());
    }

    #[test]
    fn session_state_with_complexity_score_is_pure() {
        // next_state 不修改输入（PRD §12.3 纯度约束）
        let state = RoutingSessionState {
            recent_complexity_scores: vec![0.1, 0.2],
            session_request_count: 5,
            last_recommended_slot: Some(ModelSlot::Mid),
        };
        let original = state.clone();

        let next = state.with_complexity_score(0.5, 10);

        // 原始状态未被修改
        assert_eq!(state, original);
        // 新状态包含新评分
        assert_eq!(next.recent_complexity_scores, vec![0.1, 0.2, 0.5]);
        assert_eq!(next.session_request_count, 6);
        assert_eq!(next.last_recommended_slot, Some(ModelSlot::Mid));
    }

    #[test]
    fn session_state_window_trims() {
        let state = RoutingSessionState {
            recent_complexity_scores: vec![0.1, 0.2, 0.3],
            session_request_count: 3,
            last_recommended_slot: None,
        };
        let next = state.with_complexity_score(0.4, 3);
        // 窗口大小 3：丢弃最旧值 0.1
        assert_eq!(next.recent_complexity_scores, vec![0.2, 0.3, 0.4]);
    }

    #[test]
    fn session_state_is_complexity_rising() {
        let mut state = RoutingSessionState::default();
        assert!(!state.is_complexity_rising()); // 数据不足

        state.recent_complexity_scores = vec![0.1, 0.2, 0.3, 0.5];
        assert!(state.is_complexity_rising());

        state.recent_complexity_scores = vec![0.5, 0.4, 0.3, 0.2];
        assert!(!state.is_complexity_rising());
    }

    // --- shadow_decide 确定性 ---

    #[test]
    fn shadow_decide_is_deterministic() {
        // PRD §12.3: 相同版本 + 相同输入 = 相同输出
        let input = make_test_input();
        let result1 = shadow_decide(&input, 1_700_000_000_000);
        let result2 = shadow_decide(&input, 1_700_000_000_000);
        assert_eq!(result1, result2);
    }

    #[test]
    fn shadow_decide_same_input_different_clock_same_output() {
        // stub 不使用 clock，但验证确定性（Phase 3 真实分类器若使用 clock 需另测）
        let input = make_test_input();
        let result1 = shadow_decide(&input, 1_000);
        let result2 = shadow_decide(&input, 2_000);
        assert_eq!(result1, result2);
    }

    // --- shadow_decide safe_to_execute 始终 false ---

    #[test]
    fn shadow_decide_safe_to_execute_always_false() {
        // PRD §12.5: v0.1 Shadow safe_to_execute 默认 false
        let input = make_test_input();
        let result = shadow_decide(&input, 0);
        assert!(!result.safe_to_execute);
    }

    #[test]
    fn shadow_decide_safe_to_execute_false_even_for_simple_request() {
        let input = make_simple_input();
        let result = shadow_decide(&input, 0);
        assert!(!result.safe_to_execute);
        // 即便推荐了 Cheap 档，也不执行
        assert!(result.recommended_slot.is_some());
    }

    // --- shadow_decide next_state 不修改输入 ---

    #[test]
    fn shadow_decide_does_not_mutate_input() {
        let input = make_test_input();
        let original = input.clone();
        let _result = shadow_decide(&input, 0);
        assert_eq!(input, original, "input must not be mutated by decide()");
    }

    // --- shadow_decide 推荐逻辑 ---

    #[test]
    fn shadow_decide_recommends_cheap_for_simple() {
        let input = make_simple_input();
        let result = shadow_decide(&input, 0);
        assert_eq!(result.recommended_slot, Some(ModelSlot::Cheap));
        assert!(result
            .reason_codes
            .contains(&ReasonCode::ShortUserRequest));
    }

    #[test]
    fn shadow_decide_recommends_strong_for_complex() {
        let input = make_complex_input();
        let result = shadow_decide(&input, 0);
        assert_eq!(result.recommended_slot, Some(ModelSlot::Strong));
        assert!(result
            .reason_codes
            .contains(&ReasonCode::ToolErrorPresent));
    }

    // --- DecisionResult::shadow_noop ---

    #[test]
    fn shadow_noop_is_safe() {
        let input = make_test_input();
        let result = DecisionResult::shadow_noop(&input);
        assert!(!result.safe_to_execute);
        assert_eq!(result.complexity_score, 0.0);
        assert!(result.recommended_slot.is_none());
    }

    // --- 测试辅助 ---

    fn make_test_input() -> DecisionInput {
        DecisionInput {
            decision_id: DecisionId("test-d1".to_string()),
            app_type: AppType::Claude,
            client_requested_model: "claude-sonnet-4-20250514".to_string(),
            initial_selected_provider: Some("provider-a".to_string()),
            features: RoutingFeatures::empty(
                AppType::Claude,
                "claude-sonnet-4-20250514",
                "session-hash-1",
            ),
            session_state: RoutingSessionState::default(),
            mode: RoutingMode::Shadow,
            feature_version: "v0.1".to_string(),
        }
    }

    fn make_simple_input() -> DecisionInput {
        DecisionInput {
            decision_id: DecisionId("test-simple".to_string()),
            app_type: AppType::Claude,
            client_requested_model: "claude-sonnet-4-20250514".to_string(),
            initial_selected_provider: Some("provider-a".to_string()),
            features: RoutingFeatures {
                app_type: AppType::Claude,
                original_model: "claude-sonnet-4-20250514".to_string(),
                user_message_weighted_length: 20, // short
                message_count_bucket: CountBucket::One,
                user_turn_count_bucket: CountBucket::One,
                tool_definition_count: 0,
                tool_result_count: 0,
                has_error_tool_result: false,
                constraint_count: 0,
                code_structure_score: 0.0,
                has_image_or_file: false,
                context_token_bucket: TokenBucket::Under1k,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                has_effort_or_thinking: false,
                recent_complexity_window: vec![],
                session_id_hash: super::super::SessionIdHash("h".to_string()),
                feature_version: "v0.1".to_string(),
            },
            session_state: RoutingSessionState::default(),
            mode: RoutingMode::Shadow,
            feature_version: "v0.1".to_string(),
        }
    }

    fn make_complex_input() -> DecisionInput {
        DecisionInput {
            decision_id: DecisionId("test-complex".to_string()),
            app_type: AppType::Claude,
            client_requested_model: "claude-sonnet-4-20250514".to_string(),
            initial_selected_provider: Some("provider-a".to_string()),
            features: RoutingFeatures {
                app_type: AppType::Claude,
                original_model: "claude-sonnet-4-20250514".to_string(),
                user_message_weighted_length: 500,
                message_count_bucket: CountBucket::MoreThanTwenty,
                user_turn_count_bucket: CountBucket::ElevenToTwenty,
                tool_definition_count: 15,
                tool_result_count: 8,
                has_error_tool_result: true,
                constraint_count: 8,
                code_structure_score: 0.8,
                has_image_or_file: true,
                context_token_bucket: TokenBucket::Under128k,
                cache_read_tokens: 50000,
                cache_write_tokens: 10000,
                has_effort_or_thinking: true,
                recent_complexity_window: vec![0.3, 0.4, 0.5, 0.7],
                session_id_hash: super::super::SessionIdHash("h".to_string()),
                feature_version: "v0.1".to_string(),
            },
            session_state: RoutingSessionState {
                recent_complexity_scores: vec![0.3, 0.4, 0.5, 0.7],
                session_request_count: 10,
                last_recommended_slot: Some(ModelSlot::Strong),
            },
            mode: RoutingMode::Shadow,
            feature_version: "v0.1".to_string(),
        }
    }
}

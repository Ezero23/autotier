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
        let first_half: f32 = self.recent_complexity_scores[..mid].iter().sum::<f32>() / mid as f32;
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
            classifier_version: CLASSIFIER_VERSION.to_string(),
            policy_version: POLICY_VERSION.to_string(),
        }
    }
}

/// 规则分类器版本。任何权重/阈值/规则变更必须 bump。
pub const CLASSIFIER_VERSION: &str = "rules-v0.2";

/// Shadow 策略版本。
pub const POLICY_VERSION: &str = "shadow-policy-v0.2";

// ---------------------------------------------------------------------------
// shadow_decide — 纯函数规则分类器（Phase 3）
// ---------------------------------------------------------------------------

/// Shadow 决策：纯函数规则分类器。
///
/// 约束（PRD §12.3 / §12.5）：
/// - 相同版本 + 相同输入 = 相同输出；不使用 `_clock_ms`（保留给未来时间衰减）
/// - 不修改输入；`next_state` 以新值返回
/// - v0.1 Shadow `safe_to_execute` 始终为 false；`unsafe_reasons` 如实记录风险信号
///
/// 评分模型（rules-v0.2）：各信号权重相加后 clamp 到 [0, 1]。
/// 槽位阈值：score < 0.25 → Cheap；< 0.5 → Mid；否则 Strong。
/// 显式小模型请求直接推荐 Cheap（EXPLICIT_SMALL_MODEL）。
///
/// 解析失败（`extraction_status == Unparseable`）：不推荐任何 Slot，
/// 记录 `RequestBodyUnparseable`，避免把无法解析的请求伪装成 Cheap。
pub fn shadow_decide(input: &DecisionInput, _clock_ms: u64) -> DecisionResult {
    let f = &input.features;

    // --- 解析失败短路：不推荐 Slot，不做评分 ---
    if f.extraction_status == super::features::ExtractionStatus::Unparseable {
        return DecisionResult {
            recommended_slot: None,
            complexity_score: 0.0,
            confidence: 0.0,
            reason_codes: vec![ReasonCode::ClassifierError],
            safe_to_execute: false,
            unsafe_reasons: vec![UnsafeReason::RequestBodyUnparseable],
            next_state: input.session_state.clone(),
            classifier_version: CLASSIFIER_VERSION.to_string(),
            policy_version: POLICY_VERSION.to_string(),
        };
    }

    let mut reasons = Vec::new();
    let mut unsafe_reasons = Vec::new();
    let mut score: f32 = 0.0;

    // --- 用户消息规模 ---
    if f.user_message_weighted_length < 50 {
        reasons.push(ReasonCode::ShortUserRequest);
        score -= 0.1;
    } else if f.user_message_weighted_length > 500 {
        score += 0.1;
    }

    // --- 约束密度 ---
    if f.constraint_count <= 1 {
        reasons.push(ReasonCode::LowConstraintCount);
    } else if f.constraint_count >= 5 {
        reasons.push(ReasonCode::HighConstraintCount);
        score += 0.15;
    }

    // --- Tool Loop ---
    if f.tool_result_count == 0 && f.tool_definition_count == 0 {
        reasons.push(ReasonCode::NoActiveToolLoop);
    } else if f.tool_result_count > 0 {
        // 活跃 Tool Loop：已进入多轮工具交互，复杂度上移
        score += 0.1;
    }

    if f.has_error_tool_result {
        reasons.push(ReasonCode::ToolErrorPresent);
        unsafe_reasons.push(UnsafeReason::ToolErrorPresent);
        score += 0.25;
    }

    // --- 代码结构 ---
    if f.code_structure_score > 0.6 {
        reasons.push(ReasonCode::ArchitectureSignal);
        score += 0.15;
    } else if f.code_structure_score >= 0.3 {
        reasons.push(ReasonCode::MultiFileSignal);
        score += 0.05;
    }

    // --- 多模态 ---
    if f.has_image_or_file {
        reasons.push(ReasonCode::MultimodalInput);
        score += 0.1;
    }

    // --- 上下文规模 ---
    match f.context_token_bucket {
        super::features::TokenBucket::Over128k => {
            reasons.push(ReasonCode::LongContext);
            unsafe_reasons.push(UnsafeReason::LongContextExceeded);
            score += 0.2;
        }
        super::features::TokenBucket::Under128k | super::features::TokenBucket::Under64k => {
            reasons.push(ReasonCode::LongContext);
            score += 0.15;
        }
        _ => {}
    }

    // --- 推理标记 ---
    if f.has_effort_or_thinking {
        reasons.push(ReasonCode::ReasoningSignal);
        score += 0.1;
    }

    // --- 会话趋势 ---
    if input.session_state.is_complexity_rising() {
        reasons.push(ReasonCode::RecentComplexityRising);
        score += 0.1;
    }

    // --- 缓存保护（不改分，只记录） ---
    if f.cache_read_tokens > 0 || f.cache_write_tokens > 0 {
        reasons.push(ReasonCode::CacheProtection);
    }

    let score = score.clamp(0.0, 1.0);

    // --- 推荐槽位 ---
    let explicit_small = is_explicit_small_model(&input.client_requested_model);
    if explicit_small {
        reasons.push(ReasonCode::ExplicitSmallModel);
    }
    let recommended = if explicit_small || score < 0.25 {
        Some(ModelSlot::Cheap)
    } else if score < 0.5 {
        Some(ModelSlot::Mid)
    } else {
        Some(ModelSlot::Strong)
    };

    // v0.1：能力未接入验证体系，任何候选都携带 CapabilityUnknown
    unsafe_reasons.push(UnsafeReason::CapabilityUnknown);

    // --- 置信度：信号数越多、离阈值越远，置信度越高 ---
    let signal_count = reasons.len() as f32;
    let margin = threshold_margin(score).abs();
    let confidence = (0.4 + signal_count * 0.05 + margin).clamp(0.0, 1.0);

    // --- next_state：纯函数，不修改输入 ---
    let next_state = RoutingSessionState {
        last_recommended_slot: recommended,
        ..input.session_state.with_complexity_score(score, 10)
    };

    DecisionResult {
        recommended_slot: recommended,
        complexity_score: score,
        confidence,
        reason_codes: reasons,
        // v0.1 Shadow：始终 false（PRD §12.5）
        safe_to_execute: false,
        unsafe_reasons,
        next_state,
        classifier_version: CLASSIFIER_VERSION.to_string(),
        policy_version: POLICY_VERSION.to_string(),
    }
}

/// 客户端显式请求小模型别名（haiku/mini/flash/lite 等）的粗判定。
/// 仅作信号之一，不作为能力判断的唯一来源（PRD NFR 能力不依赖名称猜测）。
fn is_explicit_small_model(model: &str) -> bool {
    let m = model.to_lowercase();
    ["haiku", "mini", "flash", "lite", "small", "turbo"]
        .iter()
        .any(|k| m.contains(k))
}

/// 当前分数到最近槽位阈值（0.25 / 0.5）的有符号距离。
fn threshold_margin(score: f32) -> f32 {
    const THRESHOLDS: [f32; 2] = [0.25, 0.5];
    THRESHOLDS
        .iter()
        .map(|t| score - t)
        .min_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap())
        .unwrap_or(0.0)
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
        assert!(result.reason_codes.contains(&ReasonCode::ShortUserRequest));
    }

    #[test]
    fn shadow_decide_recommends_strong_for_complex() {
        let input = make_complex_input();
        let result = shadow_decide(&input, 0);
        assert_eq!(result.recommended_slot, Some(ModelSlot::Strong));
        assert!(result.reason_codes.contains(&ReasonCode::ToolErrorPresent));
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

    // --- Phase 3: 真实规则引擎 ---

    #[test]
    fn explicit_small_model_forces_cheap() {
        // Forced Candidate Slot：客户端显式小模型 → Cheap + EXPLICIT_SMALL_MODEL
        let mut input = make_complex_input(); // 复杂特征也应被显式选择覆盖
        input.client_requested_model = "claude-haiku-4-5-20251001".to_string();
        let result = shadow_decide(&input, 0);
        assert_eq!(result.recommended_slot, Some(ModelSlot::Cheap));
        assert!(result
            .reason_codes
            .contains(&ReasonCode::ExplicitSmallModel));
    }

    #[test]
    fn unknown_capability_always_flagged_in_v01() {
        // Unknown Capability：v0.1 无能力验证体系，任何结果都带 CAPABILITY_UNKNOWN
        let result = shadow_decide(&make_simple_input(), 0);
        assert!(result
            .unsafe_reasons
            .contains(&UnsafeReason::CapabilityUnknown));
        assert!(!result.safe_to_execute);
    }

    #[test]
    fn tool_error_marks_unsafe() {
        let mut input = make_test_input();
        input.features.has_error_tool_result = true;
        input.features.tool_result_count = 1;
        let result = shadow_decide(&input, 0);
        assert!(result.reason_codes.contains(&ReasonCode::ToolErrorPresent));
        assert!(result
            .unsafe_reasons
            .contains(&UnsafeReason::ToolErrorPresent));
    }

    #[test]
    fn over_128k_context_marks_long_context_exceeded() {
        let mut input = make_test_input();
        input.features.context_token_bucket = TokenBucket::Over128k;
        let result = shadow_decide(&input, 0);
        assert!(result.reason_codes.contains(&ReasonCode::LongContext));
        assert!(result
            .unsafe_reasons
            .contains(&UnsafeReason::LongContextExceeded));
    }

    #[test]
    fn mid_score_recommends_mid() {
        let mut input = make_test_input();
        // 0.1(len>500 不设) 凑 0.3：tool loop +0.1, thinking +0.1, 多文件 +0.05... 用确定组合
        input.features.tool_result_count = 2; // +0.1
        input.features.has_effort_or_thinking = true; // +0.1
        input.features.code_structure_score = 0.5; // +0.05
        input.features.user_message_weighted_length = 600; // +0.1
        let result = shadow_decide(&input, 0);
        assert!((result.complexity_score - 0.35).abs() < 1e-6);
        assert_eq!(result.recommended_slot, Some(ModelSlot::Mid));
    }

    #[test]
    fn confidence_within_bounds() {
        for input in [make_simple_input(), make_test_input(), make_complex_input()] {
            let result = shadow_decide(&input, 0);
            assert!((0.0..=1.0).contains(&result.confidence));
            assert!((0.0..=1.0).contains(&result.complexity_score));
        }
    }

    #[test]
    fn next_state_records_recommended_slot() {
        let result = shadow_decide(&make_simple_input(), 0);
        assert_eq!(
            result.next_state.last_recommended_slot,
            Some(ModelSlot::Cheap)
        );
        assert_eq!(result.next_state.session_request_count, 1);
    }

    #[test]
    fn engine_versions_are_current() {
        let result = shadow_decide(&make_test_input(), 0);
        assert_eq!(result.classifier_version, CLASSIFIER_VERSION);
        assert_eq!(result.policy_version, POLICY_VERSION);
    }

    #[test]
    fn decide_p95_under_1ms() {
        let input = make_complex_input();
        const RUNS: usize = 200;
        let mut times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = std::time::Instant::now();
            let r = shadow_decide(&input, 0);
            times.push(start.elapsed());
            std::hint::black_box(r);
        }
        times.sort();
        let p95 = times[RUNS * 95 / 100];
        assert!(
            p95.as_micros() < 1000,
            "p95 decide latency {p95:?} exceeds 1ms"
        );
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
                feature_version: "claude-extractor-v0.2".to_string(),
                extraction_status: super::super::features::ExtractionStatus::Success,
            },
            session_state: RoutingSessionState::default(),
            mode: RoutingMode::Shadow,
            feature_version: "claude-extractor-v0.2".to_string(),
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
                feature_version: "claude-extractor-v0.2".to_string(),
                extraction_status: super::super::features::ExtractionStatus::Success,
            },
            session_state: RoutingSessionState {
                recent_complexity_scores: vec![0.3, 0.4, 0.5, 0.7],
                session_request_count: 10,
                last_recommended_slot: Some(ModelSlot::Strong),
            },
            mode: RoutingMode::Shadow,
            feature_version: "claude-extractor-v0.2".to_string(),
        }
    }
}

//! AutoTier 领域类型与配置契约（Phase 1）
//!
//! 本模块定义 AutoTier 的核心领域类型，所有类型为纯数据定义或纯函数，
//! 不含 I/O、数据库或网络操作。
//!
//! ## Agent-agnostic 边界（PRD §12）
//!
//! 核心类型不硬编码任何特定 Agent（Claude、Codex 等）。
//! Agent 特有的请求解析将在后续 Adapter 中实现。
//!
//! ## 四组字段语义（PRD §11.0）
//!
//! | 字段组 | 含义 |
//! |---|---|
//! | 客户端请求 | 客户端 body 中的 model；Provider Router 首次选中的 Provider |
//! | 基线出站 | 无 AutoTier 时基座本会产生的出站结果 |
//! | 候选 | Shadow 决策推荐的槽位与模型/Provider |
//! | 实际出站 | 本次请求真实发往上游的 model 与执行 Provider（含 Failover 后真值） |

// Phase 1 类型尚未被其他模块引用，Phase 2+ 将消费。
// pub use 重导出为后续 Phase 准备，当前模块私有故显式允许 unused。
#![allow(dead_code, unused_imports)]

mod decision;
mod extractor;
mod features;
mod observer;
mod writer;

pub use decision::{
    shadow_decide, DecisionInput, DecisionResult, ReasonCode, RoutingSessionState, UnsafeReason,
    CLASSIFIER_VERSION, POLICY_VERSION,
};
pub use extractor::{extract_features, FEATURE_VERSION};
pub use features::{CountBucket, RoutingFeatures, TokenBucket};
pub use observer::{build_shadow_row, is_shadow_enabled, shadow_config_for_observe, ShadowInput};
pub use writer::{
    enqueue_create, enqueue_finalize, enqueue_usage_finalize, hash_session_id,
    load_or_create_session_secret, writer_for, DecisionEvent, DecisionWriter, FinalizeEvent,
};

use crate::app_config::AppType;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AgentType — 复用基座已有的 AppType（PRD §12 Agent-agnostic）
// ---------------------------------------------------------------------------

/// Agent 类型别名，等价于基座 `AppType`。
///
/// PRD §12 要求核心类型 Agent-agnostic，基座 `AppType` 已列出
/// Claude / Codex / Gemini / GrokBuild / OpenCode / OpenClaw / Hermes，
/// 满足 Agent-agnostic 要求，此处仅提供语义别名。
pub type AgentType = AppType;

// ---------------------------------------------------------------------------
// RoutingMode（PRD §7.2 FR-MODE-001）
// ---------------------------------------------------------------------------

/// 路由模式。
///
/// v0.1 仅实现并开放 `Off` 和 `Shadow`。
/// `ExplicitOnly`、`CanaryLive`、`FullLive`、`Forced*` 为 v0.2+ 预留，
/// 定义于此供类型完整性使用，v0.1 不激活。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    /// 路由关闭：不提取特征、不记录决策、不改写请求。
    Off,
    /// Shadow 模式：提取特征、生成候选、记录决策，但 Final = Original。
    Shadow,
    // ---- v0.2+ 预留 ----
    /// 显式映射模式：仅客户端显式 model/metadata 映射，不使用启发式降档。
    ExplicitOnly,
    /// 金丝雀实路：仅 allowlist 高置信规则，按比例启用，失败一轮升级。
    CanaryLive,
    /// 全量实路：长期证据后开放。
    FullLive,
    /// 强制 Cheap 档位。
    ForcedCheap,
    /// 强制 Mid 档位。
    ForcedMid,
    /// 强制 Strong 档位。
    ForcedStrong,
}

impl Default for RoutingMode {
    /// PRD §7.2 FR-MODE-001：默认 Shadow。
    fn default() -> Self {
        Self::Shadow
    }
}

impl RoutingMode {
    /// 是否在 v0.1 中激活。
    pub fn is_v01_active(self) -> bool {
        matches!(self, Self::Off | Self::Shadow)
    }

    /// 是否为 Live 路由模式（会真实改写出站模型）。
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::CanaryLive
                | Self::FullLive
                | Self::ForcedCheap
                | Self::ForcedMid
                | Self::ForcedStrong
        )
    }
}

// ---------------------------------------------------------------------------
// ModelSlot（PRD §7.1 FR-SLOT-001）
// ---------------------------------------------------------------------------

/// Provider-specific 逻辑能力槽位。
///
/// Cheap / Mid / Strong 为必填槽位；LongContext / Background 为可选。
/// Slot 是逻辑能力档，不是全局模型名——同一 Slot 在不同 Provider 下
/// 映射到不同的 model_id。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSlot {
    /// 低成本档位。
    Cheap,
    /// 中等档位。
    Mid,
    /// 强力档位。
    Strong,
    /// 长上下文档位（可选）。
    LongContext,
    /// 后台任务档位（可选）。
    Background,
}

impl ModelSlot {
    /// 该 Slot 是否为 Shadow 启用所必需（Cheap/Mid/Strong 必填）。
    pub fn is_required_for_shadow(self) -> bool {
        matches!(self, Self::Cheap | Self::Mid | Self::Strong)
    }

    /// 稳定字符串表示，用于日志和数据库。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cheap => "cheap",
            Self::Mid => "mid",
            Self::Strong => "strong",
            Self::LongContext => "long_context",
            Self::Background => "background",
        }
    }
}

// ---------------------------------------------------------------------------
// 四个 ID 类型（PRD §11.0）
// ---------------------------------------------------------------------------

/// AutoTier 请求级内部主键，在请求入口（Handler）生成。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DecisionId(pub String);

/// 上游响应返回的真实 message id，在响应解析时捕获。可能为空。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UpstreamMessageId(pub Option<String>);

/// 基座 Usage 表去重键（`session:{message_id}`），由 Usage Logger 生成。可能为空。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UsageRequestId(pub Option<String>);

/// Session 分组评测键，入口提取后哈希。不可为空（兜底 UUID 的哈希）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionIdHash(pub String);

// ---------------------------------------------------------------------------
// 四组字段类型（PRD §11.0）
// ---------------------------------------------------------------------------

/// 客户端请求组：客户端 body 中的 model 与 Provider Router 首次选中的 Provider。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRequestFields {
    /// 客户端请求 body 中的 model 字段。
    pub client_requested_model: String,
    /// Provider Router 首次选中的 Provider ID。
    pub initial_selected_provider: Option<String>,
}

/// 基线出站组：无 AutoTier 时基座本会产生的出站结果。
///
/// 这是 Shadow 不变量的比较基准——不是客户端请求值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineOutboundFields {
    /// 无 AutoTier 时基座本会发往上游的 model。
    pub baseline_outbound_model: Option<String>,
    /// 无 AutoTier 时基座本会发往上游的 Provider。
    pub baseline_outbound_provider: Option<String>,
}

/// 候选组：Shadow 决策推荐的槽位与模型/Provider。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateFields {
    /// 推荐的逻辑槽位。
    pub recommended_slot: Option<ModelSlot>,
    /// 候选模型 ID。
    pub candidate_model: Option<String>,
    /// 候选 Provider ID。
    pub candidate_provider: Option<String>,
}

/// 实际出站组：本次请求真实发往上游的 model 与执行 Provider（含 Failover 后真值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActualOutboundFields {
    /// 实际发往上游的 model（含 Failover 后真值）。
    pub actual_outbound_model: Option<String>,
    /// 实际执行 Provider（含 Failover 后真值）。
    pub actual_outbound_provider: Option<String>,
}

// ---------------------------------------------------------------------------
// Shadow 不变量违规（PRD §7.3 FR-DEC-003）
// ---------------------------------------------------------------------------

/// Shadow 模式不变量检查失败原因。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShadowInvariantViolation {
    /// `autotier_mutated_request` 在 Shadow 模式下为 true。
    #[error("autotier_mutated_request is true in Shadow mode")]
    RequestMutated,
    /// 实际出站 model 与基线出站 model 不一致。
    #[error("actual_outbound_model ({actual:?}) != baseline_outbound_model ({baseline:?})")]
    OutboundModelMismatch {
        actual: Option<String>,
        baseline: Option<String>,
    },
    /// 实际出站 Provider 与基线出站 Provider 不一致。
    #[error("actual_outbound_provider ({actual:?}) != baseline_outbound_provider ({baseline:?})")]
    OutboundProviderMismatch {
        actual: Option<String>,
        baseline: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// RoutingDecision — 完整决策记录（PRD §11.3 逻辑 Schema 的 Rust 表示）
// ---------------------------------------------------------------------------

/// 一条完整的路由决策记录。
///
/// 组合四组字段、四个 ID、Shadow 不变量标志和决策引擎输出，
/// 用于内存表示和 Phase 2+ 的持久化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingDecision {
    // ---- 四个 ID（§11.0）----
    pub decision_id: DecisionId,
    pub session_id_hash: SessionIdHash,
    pub upstream_message_id: UpstreamMessageId,
    pub usage_request_id: UsageRequestId,

    // ---- 模式与应用类型 ----
    pub mode: RoutingMode,
    pub app_type: AgentType,

    // ---- 四组字段（§11.0）----
    pub client_request: ClientRequestFields,
    pub baseline_outbound: BaselineOutboundFields,
    pub candidate: CandidateFields,
    pub actual_outbound: ActualOutboundFields,

    // ---- Shadow 不变量标志（§11.0）----
    /// v0.1 任何请求都必须为 false。
    pub autotier_mutated_request: bool,

    // ---- 决策引擎输出（§12.2）----
    pub complexity_score: f32,
    pub confidence: f32,
    pub reason_codes: Vec<ReasonCode>,
    pub safe_to_execute: bool,
    pub unsafe_reasons: Vec<UnsafeReason>,

    // ---- 版本戳 ----
    pub feature_version: String,
    pub classifier_version: String,
    pub policy_version: String,

    // ---- 状态 ----
    pub is_complete: bool,
}

impl RoutingDecision {
    /// 检查 Shadow 不变量（PRD §7.3 FR-DEC-003）。
    ///
    /// Shadow 模式必须满足：
    /// ```text
    /// autotier_mutated_request == false
    /// actual_outbound_model    == baseline_outbound_model
    /// actual_outbound_provider == baseline_outbound_provider
    /// ```
    ///
    /// 注意：比较基线出站值，而非客户端请求值——基座自身的
    /// ModelMapping/Failover/协议转换本来就可能改变出站值。
    pub fn check_shadow_invariant(&self) -> Result<(), ShadowInvariantViolation> {
        if self.mode != RoutingMode::Shadow {
            return Ok(());
        }
        if self.autotier_mutated_request {
            return Err(ShadowInvariantViolation::RequestMutated);
        }
        if self.actual_outbound.actual_outbound_model
            != self.baseline_outbound.baseline_outbound_model
        {
            return Err(ShadowInvariantViolation::OutboundModelMismatch {
                actual: self.actual_outbound.actual_outbound_model.clone(),
                baseline: self.baseline_outbound.baseline_outbound_model.clone(),
            });
        }
        if self.actual_outbound.actual_outbound_provider
            != self.baseline_outbound.baseline_outbound_provider
        {
            return Err(ShadowInvariantViolation::OutboundProviderMismatch {
                actual: self.actual_outbound.actual_outbound_provider.clone(),
                baseline: self.baseline_outbound.baseline_outbound_provider.clone(),
            });
        }
        Ok(())
    }

    /// 便捷方法：Shadow 不变量是否满足。
    pub fn is_shadow_safe(&self) -> bool {
        self.check_shadow_invariant().is_ok()
    }
}

// ===========================================================================
// 测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- RoutingMode ---

    #[test]
    fn routing_mode_default_is_shadow() {
        assert_eq!(RoutingMode::default(), RoutingMode::Shadow);
    }

    #[test]
    fn routing_mode_v01_active() {
        assert!(RoutingMode::Off.is_v01_active());
        assert!(RoutingMode::Shadow.is_v01_active());
        assert!(!RoutingMode::CanaryLive.is_v01_active());
        assert!(!RoutingMode::FullLive.is_v01_active());
    }

    #[test]
    fn routing_mode_is_live() {
        assert!(!RoutingMode::Off.is_live());
        assert!(!RoutingMode::Shadow.is_live());
        assert!(RoutingMode::CanaryLive.is_live());
        assert!(RoutingMode::FullLive.is_live());
        assert!(RoutingMode::ForcedCheap.is_live());
    }

    #[test]
    fn routing_mode_serde_roundtrip() {
        let json = serde_json::to_string(&RoutingMode::Shadow).unwrap();
        assert_eq!(json, "\"shadow\"");
        let mode: RoutingMode = serde_json::from_str("\"off\"").unwrap();
        assert_eq!(mode, RoutingMode::Off);
    }

    // --- ModelSlot ---

    #[test]
    fn model_slot_required_for_shadow() {
        assert!(ModelSlot::Cheap.is_required_for_shadow());
        assert!(ModelSlot::Mid.is_required_for_shadow());
        assert!(ModelSlot::Strong.is_required_for_shadow());
        assert!(!ModelSlot::LongContext.is_required_for_shadow());
        assert!(!ModelSlot::Background.is_required_for_shadow());
    }

    #[test]
    fn model_slot_as_str() {
        assert_eq!(ModelSlot::Cheap.as_str(), "cheap");
        assert_eq!(ModelSlot::LongContext.as_str(), "long_context");
    }

    // --- ID newtypes ---

    #[test]
    fn decision_id_serde() {
        let id = DecisionId("req-abc123".to_string());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"req-abc123\"");
        let back: DecisionId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn upstream_message_id_nullable() {
        let none = UpstreamMessageId(None);
        let json = serde_json::to_string(&none).unwrap();
        assert_eq!(json, "null");

        let some = UpstreamMessageId(Some("msg-001".to_string()));
        let json = serde_json::to_string(&some).unwrap();
        assert_eq!(json, "\"msg-001\"");
    }

    // --- Field groups ---

    #[test]
    fn field_groups_equality() {
        let client = ClientRequestFields {
            client_requested_model: "claude-sonnet-4-20250514".to_string(),
            initial_selected_provider: Some("provider-a".to_string()),
        };
        let baseline = BaselineOutboundFields {
            baseline_outbound_model: Some("claude-sonnet-4-20250514".to_string()),
            baseline_outbound_provider: Some("provider-a".to_string()),
        };
        let actual = ActualOutboundFields {
            actual_outbound_model: Some("claude-sonnet-4-20250514".to_string()),
            actual_outbound_provider: Some("provider-a".to_string()),
        };

        // FR-DEC-003: actual_outbound == baseline_outbound
        assert_eq!(
            actual.actual_outbound_model,
            baseline.baseline_outbound_model
        );
        assert_eq!(
            actual.actual_outbound_provider,
            baseline.baseline_outbound_provider
        );
        // client_requested may differ from baseline_outbound (base ModelMapping/Failover)
        // — this is expected and NOT a violation.
        let _ = client;
    }

    // --- Shadow invariant (FR-DEC-003) ---

    #[test]
    fn shadow_invariant_passes_when_not_mutated() {
        let decision = RoutingDecision {
            decision_id: DecisionId("d1".to_string()),
            session_id_hash: SessionIdHash("hash1".to_string()),
            upstream_message_id: UpstreamMessageId(None),
            usage_request_id: UsageRequestId(None),
            mode: RoutingMode::Shadow,
            app_type: AgentType::Claude,
            client_request: ClientRequestFields {
                client_requested_model: "claude-sonnet-4-20250514".to_string(),
                initial_selected_provider: Some("p-a".to_string()),
            },
            baseline_outbound: BaselineOutboundFields {
                baseline_outbound_model: Some("claude-sonnet-4-20250514".to_string()),
                baseline_outbound_provider: Some("p-a".to_string()),
            },
            candidate: CandidateFields {
                recommended_slot: Some(ModelSlot::Cheap),
                candidate_model: Some("claude-haiku".to_string()),
                candidate_provider: Some("p-a".to_string()),
            },
            actual_outbound: ActualOutboundFields {
                actual_outbound_model: Some("claude-sonnet-4-20250514".to_string()),
                actual_outbound_provider: Some("p-a".to_string()),
            },
            autotier_mutated_request: false,
            complexity_score: 0.2,
            confidence: 0.8,
            reason_codes: vec![ReasonCode::ShortUserRequest],
            safe_to_execute: false,
            unsafe_reasons: vec![],
            feature_version: "v0.1".to_string(),
            classifier_version: "v0.1".to_string(),
            policy_version: "v0.1".to_string(),
            is_complete: false,
        };

        assert!(decision.is_shadow_safe());
        assert!(decision.check_shadow_invariant().is_ok());
    }

    #[test]
    fn shadow_invariant_fails_when_mutated() {
        let mut decision = make_shadow_decision();
        decision.autotier_mutated_request = true;

        let err = decision.check_shadow_invariant().unwrap_err();
        assert_eq!(err, ShadowInvariantViolation::RequestMutated);
    }

    #[test]
    fn shadow_invariant_fails_on_model_mismatch() {
        let mut decision = make_shadow_decision();
        decision.actual_outbound.actual_outbound_model = Some("claude-haiku-3.5".to_string());

        let err = decision.check_shadow_invariant().unwrap_err();
        assert!(matches!(
            err,
            ShadowInvariantViolation::OutboundModelMismatch { .. }
        ));
    }

    #[test]
    fn shadow_invariant_fails_on_provider_mismatch() {
        let mut decision = make_shadow_decision();
        decision.actual_outbound.actual_outbound_provider = Some("p-b".to_string());

        let err = decision.check_shadow_invariant().unwrap_err();
        assert!(matches!(
            err,
            ShadowInvariantViolation::OutboundProviderMismatch { .. }
        ));
    }

    #[test]
    fn shadow_invariant_not_checked_in_off_mode() {
        let mut decision = make_shadow_decision();
        decision.mode = RoutingMode::Off;
        // Even if mutated, Off mode doesn't check
        decision.autotier_mutated_request = true;
        assert!(decision.check_shadow_invariant().is_ok());
    }

    /// Helper: construct a valid Shadow decision for invariant tests.
    fn make_shadow_decision() -> RoutingDecision {
        RoutingDecision {
            decision_id: DecisionId("d1".to_string()),
            session_id_hash: SessionIdHash("hash1".to_string()),
            upstream_message_id: UpstreamMessageId(None),
            usage_request_id: UsageRequestId(None),
            mode: RoutingMode::Shadow,
            app_type: AgentType::Claude,
            client_request: ClientRequestFields {
                client_requested_model: "claude-sonnet-4-20250514".to_string(),
                initial_selected_provider: Some("p-a".to_string()),
            },
            baseline_outbound: BaselineOutboundFields {
                baseline_outbound_model: Some("claude-sonnet-4-20250514".to_string()),
                baseline_outbound_provider: Some("p-a".to_string()),
            },
            candidate: CandidateFields {
                recommended_slot: Some(ModelSlot::Cheap),
                candidate_model: Some("claude-haiku".to_string()),
                candidate_provider: Some("p-a".to_string()),
            },
            actual_outbound: ActualOutboundFields {
                actual_outbound_model: Some("claude-sonnet-4-20250514".to_string()),
                actual_outbound_provider: Some("p-a".to_string()),
            },
            autotier_mutated_request: false,
            complexity_score: 0.2,
            confidence: 0.8,
            reason_codes: vec![ReasonCode::ShortUserRequest],
            safe_to_execute: false,
            unsafe_reasons: vec![],
            feature_version: "v0.1".to_string(),
            classifier_version: "v0.1".to_string(),
            policy_version: "v0.1".to_string(),
            is_complete: false,
        }
    }
}

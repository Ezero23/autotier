//! RoutingFeatures — 隐私安全的派生特征（PRD §7.3 FR-DEC-001）
//!
//! 所有字段均为派生特征，不含原始 Prompt、System Prompt、Tool Schema 全文、
//! Tool Result 全文、文件内容、API Key 或 Authorization Header。

use serde::{Deserialize, Serialize};

use super::{AgentType, SessionIdHash};

// ---------------------------------------------------------------------------
// CountBucket — 消息/轮次计数分桶
// ---------------------------------------------------------------------------

/// 计数分桶，用于 Message Count 和 User Turn Count。
///
/// 分桶设计避免精确计数被反推为原始内容特征，
/// 同时保留足够的粒度供决策引擎使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CountBucket {
    #[default]
    Zero,
    One,
    TwoToFive,
    SixToTen,
    ElevenToTwenty,
    MoreThanTwenty,
}

impl CountBucket {
    /// 从精确计数映射到分桶。
    pub fn from_count(n: u32) -> Self {
        match n {
            0 => Self::Zero,
            1 => Self::One,
            2..=5 => Self::TwoToFive,
            6..=10 => Self::SixToTen,
            11..=20 => Self::ElevenToTwenty,
            _ => Self::MoreThanTwenty,
        }
    }
}

// ---------------------------------------------------------------------------
// TokenBucket — 上下文 Token 分桶
// ---------------------------------------------------------------------------

/// Token 计数分桶，用于 Context Token Bucket。
///
/// 分桶边界参考主流模型上下文窗口量级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TokenBucket {
    #[default]
    Zero,
    Under1k,
    Under4k,
    Under16k,
    Under64k,
    Under128k,
    Over128k,
}

impl TokenBucket {
    /// 从精确 token 计数映射到分桶。
    pub fn from_tokens(n: u32) -> Self {
        match n {
            0 => Self::Zero,
            1..=999 => Self::Under1k,
            1_000..=3_999 => Self::Under4k,
            4_000..=15_999 => Self::Under16k,
            16_000..=63_999 => Self::Under64k,
            64_000..=127_999 => Self::Under128k,
            _ => Self::Over128k,
        }
    }
}

// ---------------------------------------------------------------------------
// RoutingFeatures（PRD §7.3 FR-DEC-001）
// ---------------------------------------------------------------------------

/// 路由决策的隐私安全派生特征集。
///
/// 禁止默认持久化：原始 User Message、System Prompt、Tool Schema 全文、
/// Tool Result 全文、文件内容、API Key、Authorization Header、完整 Session ID。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingFeatures {
    /// 应用类型（Agent-agnostic）。
    pub app_type: AgentType,
    /// 客户端请求的原始模型（client_requested_model）。
    pub original_model: String,
    /// User Message 加权长度（中英文加权）。
    pub user_message_weighted_length: u32,
    /// 消息总数分桶。
    pub message_count_bucket: CountBucket,
    /// User 轮次计数分桶。
    pub user_turn_count_bucket: CountBucket,
    /// Tool Definition 数量。
    pub tool_definition_count: u32,
    /// Tool Result 数量。
    pub tool_result_count: u32,
    /// 是否包含 Error Tool Result。
    pub has_error_tool_result: bool,
    /// 约束条件计数（指令密度信号）。
    pub constraint_count: u32,
    /// 代码结构复杂度评分（0.0–1.0）。
    pub code_structure_score: f32,
    /// 是否包含 Image/File 输入。
    pub has_image_or_file: bool,
    /// 上下文 Token 分桶。
    pub context_token_bucket: TokenBucket,
    /// Cache Read Token 数量。
    pub cache_read_tokens: u32,
    /// Cache Write Token 数量。
    pub cache_write_tokens: u32,
    /// 是否包含 Effort/Thinking 标记。
    pub has_effort_or_thinking: bool,
    /// 最近复杂度窗口（滑动窗口，最新在前或按实现约定）。
    pub recent_complexity_window: Vec<f32>,
    /// Session ID 哈希（不保存完整 Session ID）。
    pub session_id_hash: SessionIdHash,
    /// 特征提取器版本。
    pub feature_version: String,
}

impl RoutingFeatures {
    /// 构造最小有效特征集（全零/默认值）。
    pub fn empty(app_type: AgentType, model: &str, session_hash: &str) -> Self {
        Self {
            app_type,
            original_model: model.to_string(),
            user_message_weighted_length: 0,
            message_count_bucket: CountBucket::Zero,
            user_turn_count_bucket: CountBucket::Zero,
            tool_definition_count: 0,
            tool_result_count: 0,
            has_error_tool_result: false,
            constraint_count: 0,
            code_structure_score: 0.0,
            has_image_or_file: false,
            context_token_bucket: TokenBucket::Zero,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            has_effort_or_thinking: false,
            recent_complexity_window: Vec::new(),
            session_id_hash: SessionIdHash(session_hash.to_string()),
            feature_version: "v0.1".to_string(),
        }
    }
}

// ===========================================================================
// 测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;

    #[test]
    fn count_bucket_from_count() {
        assert_eq!(CountBucket::from_count(0), CountBucket::Zero);
        assert_eq!(CountBucket::from_count(1), CountBucket::One);
        assert_eq!(CountBucket::from_count(3), CountBucket::TwoToFive);
        assert_eq!(CountBucket::from_count(8), CountBucket::SixToTen);
        assert_eq!(CountBucket::from_count(15), CountBucket::ElevenToTwenty);
        assert_eq!(CountBucket::from_count(100), CountBucket::MoreThanTwenty);
    }

    #[test]
    fn token_bucket_from_tokens() {
        assert_eq!(TokenBucket::from_tokens(0), TokenBucket::Zero);
        assert_eq!(TokenBucket::from_tokens(500), TokenBucket::Under1k);
        assert_eq!(TokenBucket::from_tokens(2000), TokenBucket::Under4k);
        assert_eq!(TokenBucket::from_tokens(10000), TokenBucket::Under16k);
        assert_eq!(TokenBucket::from_tokens(50000), TokenBucket::Under64k);
        assert_eq!(TokenBucket::from_tokens(100000), TokenBucket::Under128k);
        assert_eq!(TokenBucket::from_tokens(200000), TokenBucket::Over128k);
    }

    #[test]
    fn count_bucket_serde() {
        let json = serde_json::to_string(&CountBucket::TwoToFive).unwrap();
        assert_eq!(json, "\"two_to_five\"");
        let back: CountBucket = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CountBucket::TwoToFive);
    }

    #[test]
    fn token_bucket_serde() {
        // serde snake_case: Under128k → "under128k" (digit-letter boundary, no underscore inserted)
        let json = serde_json::to_string(&TokenBucket::Under128k).unwrap();
        assert_eq!(json, "\"under128k\"");
    }

    #[test]
    fn routing_features_empty() {
        let f = RoutingFeatures::empty(
            AppType::Claude,
            "claude-sonnet-4-20250514",
            "hashed-session-abc",
        );
        assert_eq!(f.user_message_weighted_length, 0);
        assert_eq!(f.message_count_bucket, CountBucket::Zero);
        assert!(!f.has_error_tool_result);
        assert!(f.recent_complexity_window.is_empty());
        assert_eq!(f.feature_version, "v0.1");
    }

    #[test]
    fn routing_features_serde_roundtrip() {
        let f = RoutingFeatures {
            app_type: AppType::Claude,
            original_model: "claude-sonnet-4-20250514".to_string(),
            user_message_weighted_length: 120,
            message_count_bucket: CountBucket::SixToTen,
            user_turn_count_bucket: CountBucket::TwoToFive,
            tool_definition_count: 12,
            tool_result_count: 3,
            has_error_tool_result: true,
            constraint_count: 5,
            code_structure_score: 0.7,
            has_image_or_file: false,
            context_token_bucket: TokenBucket::Under64k,
            cache_read_tokens: 8000,
            cache_write_tokens: 2000,
            has_effort_or_thinking: true,
            recent_complexity_window: vec![0.3, 0.5, 0.6],
            session_id_hash: SessionIdHash("hash-xyz".to_string()),
            feature_version: "v0.1".to_string(),
        };

        let json = serde_json::to_string(&f).unwrap();
        let back: RoutingFeatures = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn routing_features_no_raw_prompt_field() {
        // 确保 RoutingFeatures 不含原始 Prompt 字段
        let f = RoutingFeatures::empty(AppType::Claude, "model", "hash");
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("system_prompt"));
        assert!(!json.contains("raw_message"));
        assert!(!json.contains("api_key"));
        assert!(!json.contains("authorization"));
        assert!(!json.contains("tool_schema"));
        assert!(!json.contains("tool_result_full"));
    }
}

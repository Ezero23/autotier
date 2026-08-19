//! Feature Extractor — Claude Adapter（PRD §9.1 / FR-DEC-001 / AMEND-001 §6）
//!
//! 从 Anthropic Messages API 请求体提取隐私安全的派生特征（`RoutingFeatures`）。
//!
//! 纯度约束（PRD §9.1）：
//! - 纯函数；不访问数据库；不做 Provider 选择；不依赖真实时间。
//! - 相同输入必须产生相同输出（确定性）。
//!
//! 隐私约束（FR-DEC-001）：
//! - 只产出派生特征，绝不把原始 User Message / System Prompt / Tool Schema 全文 /
//!   Tool Result 全文 / 文件内容 / API Key / Authorization Header / 完整 Session ID
//!   写入输出。提取过程只在内存中读取原文做计数，计数后立即丢弃。

use serde_json::Value;

use super::features::{CountBucket, RoutingFeatures, TokenBucket};
use super::{AgentType, SessionIdHash};

/// 当前特征提取器版本。任何提取逻辑变更必须 bump。
pub const FEATURE_VERSION: &str = "claude-extractor-v0.1";

/// 上下文 token 估算除数（约 4 字符 ≈ 1 token 的经验值）。
const CHARS_PER_TOKEN: u32 = 4;

// ---------------------------------------------------------------------------
// 公开入口
// ---------------------------------------------------------------------------

/// 从 Claude Messages 请求体提取路由特征。
///
/// * `body` — 解析后的请求体 JSON（`/v1/messages` 格式）。
/// * `app_type` — 调用方应用类型。
/// * `session_hash` — 已由调用方哈希过的 Session ID（本函数不做哈希，也不见原文）。
///
/// 请求体无法解析出 messages 时，返回基于零值的最小特征集（`original_model`
/// 仍取自 body.model，缺失则为空串）——提取失败不抛错，由 Decision Engine
/// 通过 `unsafe_reasons` 表达风险。
pub fn extract_features(body: &Value, app_type: AgentType, session_hash: &str) -> RoutingFeatures {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut user_weighted_len: u32 = 0;
    let mut user_turns: u32 = 0;
    let mut tool_results: u32 = 0;
    let mut has_error_tool_result = false;
    let mut has_image_or_file = false;
    let mut constraint_count: u32 = 0;
    let mut code_block_count: u32 = 0;
    let mut file_path_count: u32 = 0;
    let mut total_chars: u64 = 0;
    let mut cache_write_chars: u64 = 0;

    for msg in &messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "user" {
            user_turns += 1;
        }

        for block in content_blocks(msg.get("content")) {
            let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
            match block_type {
                "text" => {
                    let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                    total_chars += text.len() as u64;
                    if has_cache_control(&block) {
                        cache_write_chars += text.len() as u64;
                    }
                    if role == "user" {
                        user_weighted_len += weighted_len(text);
                        constraint_count += count_constraints(text);
                        code_block_count += count_code_fences(text);
                        file_path_count += count_file_paths(text);
                    }
                }
                "tool_result" => {
                    tool_results += 1;
                    if block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        has_error_tool_result = true;
                    }
                    // tool_result 内容只计入上下文体量，不计入用户消息特征
                    total_chars += content_text_len(block.get("content"));
                }
                "image" | "document" => {
                    has_image_or_file = true;
                }
                _ => {
                    // 非标准 Content Block：只计入体量，不解读
                    total_chars += content_text_len(block.get("content"));
                }
            }
        }
    }

    // system 与 tools 只计入上下文体量与计数，不读内容
    if let Some(system) = body.get("system") {
        let system_len = match system {
            Value::String(s) => s.len() as u64,
            Value::Array(arr) => arr
                .iter()
                .map(|b| {
                    let len = content_text_len(b.get("content"))
                        + b.get("text").and_then(Value::as_str).map_or(0, str::len) as u64;
                    if has_cache_control(b) {
                        cache_write_chars += len;
                    }
                    len
                })
                .sum(),
            _ => 0,
        };
        total_chars += system_len;
    }

    let tool_definition_count = body
        .get("tools")
        .and_then(Value::as_array)
        .map_or(0, |t| t.len() as u32);
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for t in tools {
            let s = t.to_string();
            total_chars += s.len() as u64;
            if has_cache_control(t) {
                cache_write_chars += s.len() as u64;
            }
        }
    }

    let has_effort_or_thinking = body.get("thinking").is_some()
        || body.get("reasoning_effort").is_some()
        || body
            .get("metadata")
            .and_then(|m| m.get("effort"))
            .is_some();

    RoutingFeatures {
        app_type,
        original_model: model,
        user_message_weighted_length: user_weighted_len,
        message_count_bucket: CountBucket::from_count(messages.len() as u32),
        user_turn_count_bucket: CountBucket::from_count(user_turns),
        tool_definition_count,
        tool_result_count: tool_results,
        has_error_tool_result,
        constraint_count,
        code_structure_score: code_structure_score(code_block_count, file_path_count),
        has_image_or_file,
        context_token_bucket: TokenBucket::from_tokens((total_chars / CHARS_PER_TOKEN as u64) as u32),
        cache_read_tokens: 0, // 请求到达时未知，由 Usage Finalize 阶段回填
        cache_write_tokens: (cache_write_chars / CHARS_PER_TOKEN as u64) as u32,
        has_effort_or_thinking,
        recent_complexity_window: Vec::new(), // 由 Decision Engine 经 Session State 注入
        session_id_hash: SessionIdHash(session_hash.to_string()),
        feature_version: FEATURE_VERSION.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 内部辅助（全部为纯函数）
// ---------------------------------------------------------------------------

/// 把 content 字段统一为 block 迭代：字符串包装成单个 text block，数组原样返回。
fn content_blocks(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::String(s)) => vec![Value::Object(serde_json::Map::from_iter([(
            "type".to_string(),
            Value::String("text".to_string()),
        ), (
            "text".to_string(),
            Value::String(s.clone()),
        )]))],
        Some(Value::Array(arr)) => arr.clone(),
        _ => Vec::new(),
    }
}

/// content 内文本长度（字符串或 text block 数组）。
fn content_text_len(content: Option<&Value>) -> u64 {
    match content {
        Some(Value::String(s)) => s.len() as u64,
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|b| b.get("text").and_then(Value::as_str).map_or(0, str::len) as u64)
            .sum(),
        _ => 0,
    }
}

fn has_cache_control(block: &Value) -> bool {
    block.get("cache_control").is_some()
}

/// 中英文加权长度：CJK 字符按 2 计，其余按 1 计。
fn weighted_len(text: &str) -> u32 {
    text.chars()
        .map(|c| if is_cjk(c) { 2u32 } else { 1u32 })
        .sum()
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK 统一表意文字
        | '\u{3400}'..='\u{4DBF}' // 扩展 A
        | '\u{3000}'..='\u{303F}' // CJK 标点
        | '\u{FF00}'..='\u{FFEF}' // 全角
    )
}

/// 约束条件计数：中英文强约束关键词出现次数（指令密度信号）。
fn count_constraints(text: &str) -> u32 {
    const KEYWORDS: &[&str] = &[
        "必须", "不要", "禁止", "不得", "只能", "务必", "严格",
        "must", "do not", "don't", "never", "always", "only", "strictly",
    ];
    let lower = text.to_lowercase();
    KEYWORDS
        .iter()
        .map(|k| lower.matches(k).count() as u32)
        .sum()
}

/// 代码块围栏（```）数量。
fn count_code_fences(text: &str) -> u32 {
    text.matches("```").count() as u32 / 2
}

/// 文件路径信号计数（粗粒度：含 `/` 且以常见源码扩展名结尾的 token）。
fn count_file_paths(text: &str) -> u32 {
    const EXTS: &[&str] = &[".rs", ".ts", ".tsx", ".js", ".py", ".go", ".java", ".md", ".json", ".toml"];
    text.split_whitespace()
        .filter(|tok| {
            tok.contains('/') && EXTS.iter().any(|e| tok.trim_end_matches([',', ')', ']', '`', '"', '\'']).ends_with(e))
        })
        .count() as u32
}

/// 代码结构复杂度评分（0.0–1.0）：代码块与文件路径信号的加权和，封顶 1.0。
fn code_structure_score(code_blocks: u32, file_paths: u32) -> f32 {
    let raw = code_blocks as f32 * 0.15 + file_paths as f32 * 0.1;
    raw.clamp(0.0, 1.0)
}

// ===========================================================================
// 测试（PRD §17.1 Feature Extractor 边界清单）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use serde_json::json;

    fn extract(body: Value) -> RoutingFeatures {
        extract_features(&body, AppType::Claude, "session-hash")
    }

    // --- 中英文长度 ---

    #[test]
    fn weighted_length_counts_cjk_double() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "你好 world"}]
        });
        let f = extract(body);
        // "你好"=2 CJK ×2 = 4，空格=1，"world"=5 → 10
        assert_eq!(f.user_message_weighted_length, 10);
    }

    // --- 空消息 ---

    #[test]
    fn empty_messages_produce_zero_features() {
        let body = json!({"model": "m", "messages": []});
        let f = extract(body);
        assert_eq!(f.message_count_bucket, CountBucket::Zero);
        assert_eq!(f.user_turn_count_bucket, CountBucket::Zero);
        assert_eq!(f.user_message_weighted_length, 0);
        assert_eq!(f.context_token_bucket, TokenBucket::Zero);
    }

    #[test]
    fn missing_messages_field_does_not_panic() {
        let f = extract(json!({"model": "m"}));
        assert_eq!(f.original_model, "m");
        assert_eq!(f.message_count_bucket, CountBucket::Zero);
    }

    // --- 多轮 ---

    #[test]
    fn multi_turn_counts_only_user_turns() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "user", "content": "第一问"},
                {"role": "assistant", "content": "回答一"},
                {"role": "user", "content": "第二问"},
                {"role": "assistant", "content": "回答二"},
                {"role": "user", "content": "第三问"}
            ]
        });
        let f = extract(body);
        assert_eq!(f.message_count_bucket, CountBucket::TwoToFive);
        assert_eq!(f.user_turn_count_bucket, CountBucket::TwoToFive);
    }

    // --- Tool Definition ---

    #[test]
    fn counts_tool_definitions() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "a"}, {"name": "b"}, {"name": "c"}]
        });
        assert_eq!(extract(body).tool_definition_count, 3);
    }

    // --- Tool Result Error ---

    #[test]
    fn detects_error_tool_result() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "ok"},
                    {"type": "tool_result", "tool_use_id": "t2", "is_error": true, "content": "boom"}
                ]}
            ]
        });
        let f = extract(body);
        assert_eq!(f.tool_result_count, 2);
        assert!(f.has_error_tool_result);
    }

    // --- 代码块与文件路径 ---

    #[test]
    fn code_structure_score_from_fences_and_paths() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "改一下 src/main.rs 和 lib/a.ts\n```rust\nfn main() {}\n```"}]
        });
        let f = extract(body);
        // 1 个代码块(0.15) + 2 个文件路径(0.2) = 0.35
        assert!((f.code_structure_score - 0.35).abs() < 1e-6);
    }

    // --- 多模态 ---

    #[test]
    fn detects_image_and_document_blocks() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": [
                {"type": "image", "source": {"type": "base64", "data": "..."}}
            ]}]
        });
        assert!(extract(body).has_image_or_file);
    }

    // --- 超长输入 ---

    #[test]
    fn long_input_maps_to_high_token_bucket() {
        let long_text = "a".repeat(600_000); // ≈150k tokens
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": long_text}]
        });
        assert_eq!(extract(body).context_token_bucket, TokenBucket::Over128k);
    }

    // --- 非标准 Content Block ---

    #[test]
    fn nonstandard_block_does_not_panic() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": [
                {"type": "unknown_future_block", "content": "x".repeat(100)}
            ]}]
        });
        let f = extract(body);
        assert!(!f.has_image_or_file);
        assert_ne!(f.context_token_bucket, TokenBucket::Zero);
    }

    // --- 约束计数 ---

    #[test]
    fn constraint_keywords_counted_bilingually() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "必须改，不要破坏现有测试。You must not skip docs."}]
        });
        let f = extract(body);
        // 必须(1) + 不要(1) + must(1) + not→do not 不计；"must" 命中 1
        assert!(f.constraint_count >= 3, "got {}", f.constraint_count);
    }

    // --- Thinking / Effort 标记 ---

    #[test]
    fn detects_thinking_block() {
        let body = json!({
            "model": "m",
            "messages": [],
            "thinking": {"type": "enabled", "budget_tokens": 1024}
        });
        assert!(extract(body).has_effort_or_thinking);
    }

    // --- Cache Write 估算 ---

    #[test]
    fn cache_control_marks_estimated_as_cache_write() {
        let text = "x".repeat(4000); // ≈1000 tokens
        let body = json!({
            "model": "m",
            "system": [{"type": "text", "text": text, "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": "hi"}]
        });
        let f = extract(body);
        assert_eq!(f.cache_write_tokens, 1000);
        assert_eq!(f.cache_read_tokens, 0);
    }

    // --- 确定性 ---

    #[test]
    fn extraction_is_deterministic() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "你好，改 src/a.rs。必须保持兼容。"}],
            "tools": [{"name": "t"}]
        });
        assert_eq!(extract(body.clone()), extract(body));
    }

    // --- Raw Prompt 不进入持久化结构 ---

    #[test]
    fn output_contains_no_raw_content() {
        let secret = "这是一段不应出现在特征里的原文-s3cr3t";
        let body = json!({
            "model": "m",
            "system": "system prompt 原文",
            "messages": [{"role": "user", "content": secret}],
            "tools": [{"name": "tool", "description": "工具原文"}]
        });
        let f = extract(body);
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains(secret));
        assert!(!json.contains("system prompt 原文"));
        assert!(!json.contains("工具原文"));
        assert!(!json.contains("s3cr3t"));
    }

    // --- 性能：p95 < 1ms（PRD Phase 3 Exit Gate） ---

    #[test]
    fn extraction_p95_under_1ms() {
        // 构造一个接近真实 Claude Code 规模的中等请求
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "y".repeat(20_000),
            "messages": (0..30).map(|i| json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": format!("第 {} 轮：修改 src/mod_{}.rs，必须兼容。\n```rust\nfn f() {{}}\n```", i, i)
            })).collect::<Vec<_>>(),
            "tools": (0..15).map(|i| json!({"name": format!("tool_{i}"), "description": "d".repeat(200)})).collect::<Vec<_>>()
        });

        const RUNS: usize = 200;
        let mut times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let body_copy = body.clone(); // 克隆不计入提取耗时
            let start = std::time::Instant::now();
            let f = extract(body_copy);
            times.push(start.elapsed());
            std::hint::black_box(f);
        }
        times.sort();
        let p95 = times[RUNS * 95 / 100];
        assert!(
            p95.as_micros() < 1000,
            "p95 extraction latency {:?} exceeds 1ms",
            p95
        );
    }
}

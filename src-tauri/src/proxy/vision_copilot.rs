//! Vision Copilot: convert image blocks into text for text-only primary models.
//!
//! This module deliberately does not guess unknown model capabilities. It only
//! runs when the provider declares the primary model text-only or the model is
//! in the user-maintained exact-name list.

use crate::database::AutotierRoutingConfigDto;
use crate::model_capabilities::{
    find_declared_vision_model, find_known_vision_model, image_input_capability_from_settings,
    ImageInputCapability,
};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use crate::proxy::forwarder::RequestForwarder;
use crate::proxy::handler_context::RequestContext;
use crate::proxy::hyper_client::MAX_RESPONSE_BODY_BYTES;
use crate::proxy::media_sanitizer::contains_image_blocks;
use axum::http::{Extensions, HeaderMap, Method};
use bytes::Bytes;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

const DESCRIPTION_PROMPT: &str = "请详细描述图片内容，面向编程和工程问题。图片中的文字请尽量逐字转抄，同时说明布局、颜色、控件位置、错误码、日志和其他可能影响判断的细节。只输出图片描述，不要回答用户问题。";
const DEFAULT_MAX_TOKENS: u64 = 1600;

static DESCRIPTION_CACHE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Default)]
pub struct VisionDescribeUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct VisionApplyResult {
    pub applied: bool,
    pub usage: VisionDescribeUsage,
}

pub struct VisionForwardContext<'a> {
    pub forwarder: &'a RequestForwarder,
    pub method: &'a Method,
    pub endpoint: &'a str,
    pub headers: &'a HeaderMap,
    pub providers: Vec<Provider>,
}

pub fn should_run(
    body: &Value,
    provider: &Provider,
    config: &AutotierRoutingConfigDto,
    model: &str,
) -> bool {
    if !config.vision_copilot_enabled || !contains_image_blocks(body) {
        return false;
    }
    if config.vision_copilot_model.trim().is_empty()
        && find_declared_vision_model(&provider.settings_config).is_none()
        && find_known_vision_model(&provider.settings_config).is_none()
    {
        return false;
    }
    is_configured_text_only(model, provider, config)
}

fn is_configured_text_only(
    model: &str,
    provider: &Provider,
    config: &AutotierRoutingConfigDto,
) -> bool {
    if config
        .vision_text_only_models
        .iter()
        .any(|candidate| model_ids_match(candidate, model))
    {
        return true;
    }
    image_input_capability_from_settings(&provider.settings_config, model, true)
        == ImageInputCapability::Unsupported
}

pub async fn apply(
    body: &mut Value,
    ctx: &RequestContext,
    forward: VisionForwardContext<'_>,
    config: &AutotierRoutingConfigDto,
) -> Result<VisionApplyResult, ProxyError> {
    if !should_run(body, &ctx.provider, config, &ctx.request_model) {
        return Ok(VisionApplyResult {
            applied: false,
            usage: VisionDescribeUsage::default(),
        });
    }

    let assistant_model = config
        .vision_copilot_model
        .trim()
        .strip_suffix("[1M]")
        .unwrap_or(config.vision_copilot_model.trim())
        .trim();
    let assistant_model = if assistant_model.is_empty() {
        find_declared_vision_model(&ctx.provider.settings_config)
            .or_else(|| find_known_vision_model(&ctx.provider.settings_config))
            .ok_or_else(|| ProxyError::ConfigError("未找到已声明支持图片的视觉助手模型".into()))?
    } else {
        assistant_model.to_string()
    };

    let image_blocks = collect_image_blocks(body);
    if image_blocks.is_empty() {
        return Ok(VisionApplyResult {
            applied: false,
            usage: VisionDescribeUsage::default(),
        });
    }
    let fingerprint = fingerprint_blocks(&image_blocks);
    let (description, usage) = if let Some(cached) = cached_description(&fingerprint) {
        (cached, VisionDescribeUsage::default())
    } else {
        let mut describe_content = image_blocks.clone();
        describe_content.push(json!({"type": "text", "text": DESCRIPTION_PROMPT}));
        let describe_body = json!({
            "model": assistant_model,
            "max_tokens": DEFAULT_MAX_TOKENS,
            "stream": false,
            "messages": [{
                "role": "user",
                "content": describe_content
            }]
        });
        let result = forward
            .forwarder
            .forward_with_retry(
                &ctx.app_type,
                forward.method.clone(),
                forward.endpoint,
                describe_body,
                forward.headers.clone(),
                Extensions::new(),
                forward.providers,
            )
            .await
            .map_err(|error| error.error)?;
        let response = result.response;
        let raw = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await?;
        let usage = response_usage_from_bytes(&raw);
        let description = extract_description(&raw)
            .ok_or_else(|| ProxyError::ForwardFailed("视觉助手没有返回可用的图片描述".into()))?;
        store_description(fingerprint, description.clone());
        (description, usage)
    };

    let replaced = replace_image_blocks(body, &description);
    if replaced == 0 {
        return Ok(VisionApplyResult {
            applied: false,
            usage,
        });
    }
    Ok(VisionApplyResult {
        applied: true,
        usage,
    })
}

fn collect_image_blocks(body: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            if message.get("role").and_then(Value::as_str) == Some("tool") {
                continue;
            }
            if let Some(content) = message.get("content") {
                collect_images(content, &mut blocks);
            }
        }
    }
    blocks
}

fn collect_images(value: &Value, output: &mut Vec<Value>) {
    if let Some(blocks) = value.as_array() {
        for block in blocks {
            if is_image_block(block) {
                output.push(block.clone());
            } else if let Some(content) = block.get("content") {
                collect_images(content, output);
            }
        }
    }
}

fn replace_image_blocks(body: &mut Value, description: &str) -> usize {
    let replacement = json!({
        "type": "text",
        "text": format!("[图片内容描述]\n{description}")
    });
    let mut replaced = 0;
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if let Some(content) = message.get_mut("content") {
                replaced += replace_images(content, &replacement);
            }
        }
    }
    replaced
}

fn replace_images(value: &mut Value, replacement: &Value) -> usize {
    let Some(blocks) = value.as_array_mut() else {
        return 0;
    };
    let mut replaced = 0;
    for block in blocks {
        if is_image_block(block) {
            *block = replacement.clone();
            replaced += 1;
        } else if let Some(content) = block.get_mut("content") {
            replaced += replace_images(content, replacement);
        }
    }
    replaced
}

fn is_image_block(value: &Value) -> bool {
    matches!(
        value.get("type").and_then(Value::as_str),
        Some("image" | "image_url")
    )
}

fn fingerprint_blocks(blocks: &[Value]) -> String {
    let raw = serde_json::to_vec(blocks).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(raw);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn cached_description(key: &str) -> Option<String> {
    DESCRIPTION_CACHE.lock().ok()?.get(key).cloned()
}

fn store_description(key: String, description: String) {
    if let Ok(mut cache) = DESCRIPTION_CACHE.lock() {
        if cache.len() >= 256 {
            if let Some(first) = cache.keys().next().cloned() {
                cache.remove(&first);
            }
        }
        cache.insert(key, description);
    }
}

fn response_usage_from_bytes(raw: &Bytes) -> VisionDescribeUsage {
    let Ok(value) = serde_json::from_slice::<Value>(raw) else {
        return VisionDescribeUsage::default();
    };
    let usage = value.get("usage").or_else(|| value.get("usageMetadata"));
    VisionDescribeUsage {
        input_tokens: usage
            .and_then(|value| {
                value
                    .get("input_tokens")
                    .or_else(|| value.get("promptTokenCount"))
            })
            .and_then(Value::as_i64),
        output_tokens: usage
            .and_then(|value| {
                value
                    .get("output_tokens")
                    .or_else(|| value.get("candidatesTokenCount"))
            })
            .and_then(Value::as_i64),
    }
}

fn extract_description(raw: &Bytes) -> Option<String> {
    let value: Value = serde_json::from_slice(raw).ok()?;
    if let Some(text) = value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| {
            let text = items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        })
    {
        return Some(text);
    }
    value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string)
}

fn model_ids_match(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .trim()
            .trim_start_matches("models/")
            .trim_end_matches("[1M]")
            .trim()
            .to_ascii_lowercase()
    };
    let left = normalize(left);
    let right = normalize(right);
    left == right || left.rsplit('/').next() == right.rsplit('/').next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_images_but_keeps_user_text() {
        let mut body = json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "这是什么错误？"},
            {"type": "image", "source": {"type": "base64", "data": "abc"}}
        ]}]});
        let count = replace_image_blocks(&mut body, "截图显示端口 3000 被拒绝");
        assert_eq!(count, 1);
        assert_eq!(body["messages"][0]["content"][0]["text"], "这是什么错误？");
        assert!(body.to_string().contains("端口 3000"));
        assert!(!contains_image_blocks(&body));
    }

    #[test]
    fn extracts_anthropic_and_openai_text() {
        assert_eq!(
            extract_description(&Bytes::from(r#"{"content":[{"type":"text","text":"ok"}]}"#))
                .as_deref(),
            Some("ok")
        );
        assert_eq!(
            extract_description(&Bytes::from(
                r#"{"choices":[{"message":{"content":"ok"}}]}"#
            ))
            .as_deref(),
            Some("ok")
        );
    }
}

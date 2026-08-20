//! Phase 4D 抓包对比：批准白名单与差异报告。
//!
//! 白名单只覆盖与 AutoTier 无关的动态字段（端口、时钟、生成 ID）。
//! model / provider / method / path / body / SSE 事件不得进入白名单。

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value;

/// 上游请求头：不同 mock 监听端口导致 Host 不同。
pub const WHITELIST_UPSTREAM_HEADERS: &[&str] = &["host"];

/// 客户端响应头：由 HTTP 栈生成。
pub const WHITELIST_CLIENT_HEADERS: &[&str] = &["date"];

/// 基座 Usage 表：时钟与错误路径生成的 request_id。
pub const WHITELIST_USAGE_FIELDS: &[&str] = &[
    "request_id",
    "latency_ms",
    "first_token_ms",
    "duration_ms",
    "created_at",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub path: String,
    pub left: String,
    pub right: String,
}

impl Diff {
    pub fn new(path: impl Into<String>, left: impl Into<String>, right: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            left: left.into(),
            right: right.into(),
        }
    }
}

pub fn is_whitelisted(path: &str, whitelist: &[&str]) -> bool {
    let last = path.rsplit('.').next().unwrap_or(path);
    whitelist.iter().any(|w| *w == last || *w == path)
}

pub fn diff_str(path: &str, left: &str, right: &str, whitelist: &[&str]) -> Vec<Diff> {
    if left == right || is_whitelisted(path, whitelist) {
        Vec::new()
    } else {
        vec![Diff::new(path, left, right)]
    }
}

pub fn diff_headers(
    prefix: &str,
    left: &BTreeMap<String, String>,
    right: &BTreeMap<String, String>,
    whitelist: &[&str],
) -> Vec<Diff> {
    let mut diffs = Vec::new();
    let mut keys: Vec<&String> = left.keys().chain(right.keys()).collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        let path = format!("{prefix}.{key}");
        if is_whitelisted(key, whitelist) || is_whitelisted(&path, whitelist) {
            continue;
        }
        let l = left.get(key).map(String::as_str).unwrap_or("<missing>");
        let r = right.get(key).map(String::as_str).unwrap_or("<missing>");
        if redact_header(key) {
            if l != r {
                diffs.push(Diff::new(
                    &path,
                    "<redacted-mismatch>",
                    "<redacted-mismatch>",
                ));
            }
            continue;
        }
        diffs.extend(diff_str(&path, l, r, &[]));
    }
    diffs
}

fn redact_header(name: &str) -> bool {
    matches!(
        name,
        "authorization" | "x-api-key" | "proxy-authorization" | "cookie"
    )
}

/// Mock 上游生成的 message id / tool id；Off vs Shadow 对比时允许不同。
pub const WHITELIST_CLIENT_BODY_FIELDS: &[&str] = &["id"];

pub fn diff_json_parity(prefix: &str, left: &Value, right: &Value) -> Vec<Diff> {
    diff_json_with_skip(prefix, left, right, WHITELIST_CLIENT_BODY_FIELDS)
}

pub fn diff_json(prefix: &str, left: &Value, right: &Value) -> Vec<Diff> {
    diff_json_with_skip(prefix, left, right, &[])
}

fn diff_json_with_skip(prefix: &str, left: &Value, right: &Value, skip_keys: &[&str]) -> Vec<Diff> {
    if left == right {
        return Vec::new();
    }
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            let mut diffs = Vec::new();
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                if skip_keys.iter().any(|s| *s == key.as_str()) {
                    continue;
                }
                let path = format!("{prefix}.{key}");
                match (a.get(key), b.get(key)) {
                    (Some(lv), Some(rv)) => {
                        diffs.extend(diff_json_with_skip(&path, lv, rv, skip_keys))
                    }
                    (Some(lv), None) => diffs.push(Diff::new(&path, compact(lv), "<missing>")),
                    (None, Some(rv)) => diffs.push(Diff::new(&path, "<missing>", compact(rv))),
                    (None, None) => {}
                }
            }
            diffs
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b.iter())
            .enumerate()
            .flat_map(|(i, (lv, rv))| {
                diff_json_with_skip(&format!("{prefix}[{i}]"), lv, rv, skip_keys)
            })
            .collect(),
        _ => vec![Diff::new(prefix, compact(left), compact(right))],
    }
}

fn compact(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| v.to_string())
}

pub fn format_report(title: &str, diffs: &[Diff]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "=== {title}: {} diffs ===", diffs.len());
    for d in diffs {
        let _ = writeln!(out, "  {} | left={} | right={}", d.path, d.left, d.right);
    }
    out
}

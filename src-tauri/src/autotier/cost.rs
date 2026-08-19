//! Phase 5B：Decimal 实际成本、Cache Write TTL 完整性、Candidate Low/Base/High。
//!
//! 金额以 `Decimal` 计算、以字符串落库，禁止用 f64 作为账面 Source of Truth。
//! 未知 TTL 不得把 `cache_creation_tokens` 武断记入 5m 或 1h。
//! Candidate 投影不得写入 `actual_cost_usd`。

use std::collections::BTreeSet;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `cost_assumptions_json` 最大长度（字节）。超限截断并打标。
pub const COST_ASSUMPTIONS_MAX_LEN: usize = 8192;
pub const COST_SCHEMA_VERSION: u32 = 1;

/// Phase 5B 首次交付的三类版本戳。历史 Decision 必须引用写入当日的值。
pub const CAPABILITY_TABLE_VERSION: &str = "capability-table-v0.1";
pub const COST_MODEL_VERSION: &str = "cost-model-v0.1";
pub const CACHE_STATS_VERSION: &str = "cache-stats-v0.1";

const MILLION: i64 = 1_000_000;
/// High 估计：相对实际输出再增加 50%。
const HIGH_OUTPUT_UPLIFT: &str = "0.5";

pub const ASSUMPTION_CACHE_HIT_PRESERVED: &str = "CACHE_HIT_PRESERVED";
pub const ASSUMPTION_TTL_5M: &str = "CACHE_WRITE_TTL_5M";
pub const ASSUMPTION_TTL_1H: &str = "CACHE_WRITE_TTL_1H";
pub const ASSUMPTION_TTL_UNKNOWN: &str = "CACHE_WRITE_TTL_UNKNOWN";
pub const ASSUMPTION_PRICE_MISSING: &str = "PRICE_MISSING";
pub const ASSUMPTION_PRICE_FROZEN: &str = "PRICE_SNAPSHOT_FROZEN";
pub const ASSUMPTION_WRITE_PRICE_COMBINED: &str = "CACHE_WRITE_PRICE_COMBINED";
pub const ASSUMPTION_PARSE_FAILED: &str = "ASSUMPTIONS_PARSE_FAILED";
pub const ASSUMPTION_TRUNCATED: &str = "ASSUMPTIONS_TRUNCATED";
pub const ASSUMPTION_HISTORICAL_MEDIAN_UNAVAILABLE: &str = "HISTORICAL_MEDIAN_UNAVAILABLE";
pub const ASSUMPTION_CANDIDATE_USED_BASELINE_PRICES: &str = "CANDIDATE_USED_BASELINE_PRICES";
pub const ASSUMPTION_SHADOW_NO_SAVING: &str = "SHADOW_NO_REALIZED_SAVING";
pub const ASSUMPTION_CACHE_BUST_HIGH: &str = "CACHE_BUST_HIGH_ESTIMATE";
pub const ASSUMPTION_RETRY_REPLAY: &str = "RETRY_COST_REPLAY_ESTIMATE";

/// 请求 Cache Policy 可归因的 Write TTL。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheWriteTtl {
    FiveMin,
    OneHour,
    Unknown,
}

impl CacheWriteTtl {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveMin => "5m",
            Self::OneHour => "1h",
            Self::Unknown => "unknown",
        }
    }

    pub fn assumption(self) -> &'static str {
        match self {
            Self::FiveMin => ASSUMPTION_TTL_5M,
            Self::OneHour => ASSUMPTION_TTL_1H,
            Self::Unknown => ASSUMPTION_TTL_UNKNOWN,
        }
    }

    pub fn parse_label(s: &str) -> Self {
        match s {
            "5m" => Self::FiveMin,
            "1h" => Self::OneHour,
            _ => Self::Unknown,
        }
    }
}

/// 价格快照一条腿（Baseline 或 Candidate）。单价均为字符串。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceLeg {
    pub provider_id: Option<String>,
    pub model_id: String,
    pub price_source: String,
    pub price_observed_at: i64,
    pub input_per_million: String,
    pub output_per_million: String,
    pub cache_read_per_million: String,
    pub cache_write_5m_per_million: String,
    pub cache_write_1h_per_million: String,
}

impl PriceLeg {
    pub fn rates(&self) -> Option<PriceRates> {
        Some(PriceRates {
            input: parse_decimal(&self.input_per_million)?,
            output: parse_decimal(&self.output_per_million)?,
            cache_read: parse_decimal(&self.cache_read_per_million)?,
            cache_write_5m: parse_decimal(&self.cache_write_5m_per_million)?,
            cache_write_1h: parse_decimal(&self.cache_write_1h_per_million)?,
        })
    }
}

/// 已解析的百万 token 单价。
#[derive(Debug, Clone, Copy)]
pub struct PriceRates {
    pub input: Decimal,
    pub output: Decimal,
    pub cache_read: Decimal,
    pub cache_write_5m: Decimal,
    pub cache_write_1h: Decimal,
}

impl PriceRates {
    pub fn write_for_ttl(self, ttl: CacheWriteTtl) -> Decimal {
        match ttl {
            CacheWriteTtl::OneHour => self.cache_write_1h,
            CacheWriteTtl::FiveMin | CacheWriteTtl::Unknown => self.cache_write_5m,
        }
    }
}

/// 持久化到 `cost_assumptions_json` 的快照文档。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostAssumptions {
    #[serde(default = "cost_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub baseline: Option<PriceLeg>,
    #[serde(default)]
    pub candidate: Option<PriceLeg>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default = "unknown_ttl_label")]
    pub cache_write_ttl: String,
    #[serde(default = "partial_coverage")]
    pub breakdown_coverage: String,
    #[serde(default)]
    pub capability_table_version: String,
    #[serde(default)]
    pub cost_model_version: String,
    #[serde(default)]
    pub cache_stats_version: String,
}

fn cost_schema_version() -> u32 {
    COST_SCHEMA_VERSION
}
fn unknown_ttl_label() -> String {
    CacheWriteTtl::Unknown.as_str().to_string()
}
fn partial_coverage() -> String {
    "partial".to_string()
}

impl Default for CostAssumptions {
    fn default() -> Self {
        Self {
            schema_version: COST_SCHEMA_VERSION,
            baseline: None,
            candidate: None,
            assumptions: Vec::new(),
            cache_write_ttl: unknown_ttl_label(),
            breakdown_coverage: partial_coverage(),
            capability_table_version: String::new(),
            cost_model_version: String::new(),
            cache_stats_version: String::new(),
        }
    }
}

/// 空字段填入当日版本常量；已有值不覆盖（历史 Decision 可复现）。
pub fn stamp_model_versions(doc: &mut CostAssumptions) {
    if doc.capability_table_version.is_empty() {
        doc.capability_table_version = CAPABILITY_TABLE_VERSION.to_string();
    }
    if doc.cost_model_version.is_empty() {
        doc.cost_model_version = COST_MODEL_VERSION.to_string();
    }
    if doc.cache_stats_version.is_empty() {
        doc.cache_stats_version = CACHE_STATS_VERSION.to_string();
    }
}

/// 一次请求的计费 token 与重试计数。
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenCounts {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub retry_count: i32,
    pub fallback_count: i32,
}

/// Candidate Low / Base / High（字符串金额）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateRange {
    pub low_usd: Option<String>,
    pub base_usd: Option<String>,
    pub high_usd: Option<String>,
}

/// `evaluate_costs` 的完整输出。
#[derive(Debug, Clone)]
pub struct CostOutcome {
    pub actual_usd: Option<String>,
    pub candidate: CandidateRange,
    pub write_5m: Option<i64>,
    pub write_1h: Option<i64>,
    pub assumptions: CostAssumptions,
}

pub fn price_leg_is_frozen(leg: Option<&PriceLeg>) -> bool {
    let Some(leg) = leg else {
        return false;
    };
    if leg.price_source == "unknown" {
        return false;
    }
    leg.rates().is_some()
}

pub fn push_assumption(doc: &mut CostAssumptions, code: &str) {
    if !doc.assumptions.iter().any(|a| a == code) {
        doc.assumptions.push(code.to_string());
    }
}

pub fn ttl_from_assumptions(doc: &CostAssumptions) -> CacheWriteTtl {
    CacheWriteTtl::parse_label(&doc.cache_write_ttl)
}

/// 扫描请求体 `cache_control`。多种 TTL 并存或无法识别时为 Unknown。
pub fn classify_cache_write_ttl(body: &Value) -> CacheWriteTtl {
    let mut found = BTreeSet::new();
    collect_cache_ttls(body, &mut found);
    match found.len() {
        0 => CacheWriteTtl::Unknown,
        1 => found.into_iter().next().unwrap_or(CacheWriteTtl::Unknown),
        _ => CacheWriteTtl::Unknown,
    }
}

fn collect_cache_ttls(value: &Value, out: &mut BTreeSet<CacheWriteTtl>) {
    match value {
        Value::Object(map) => {
            if let Some(cc) = map.get("cache_control") {
                out.insert(ttl_from_cache_control(cc));
            }
            for v in map.values() {
                collect_cache_ttls(v, out);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_cache_ttls(v, out);
            }
        }
        _ => {}
    }
}

fn ttl_from_cache_control(cc: &Value) -> CacheWriteTtl {
    let ttl = cc
        .get("ttl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match ttl.as_str() {
        "1h" | "3600" | "3600s" => CacheWriteTtl::OneHour,
        "5m" | "300" | "300s" => CacheWriteTtl::FiveMin,
        "" => {
            // Anthropic 默认 ephemeral = 5m。
            if cc
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.eq_ignore_ascii_case("ephemeral"))
                || cc.is_object()
            {
                CacheWriteTtl::FiveMin
            } else {
                CacheWriteTtl::Unknown
            }
        }
        _ => CacheWriteTtl::Unknown,
    }
}

/// 按 TTL 归因 cache creation。Unknown 时 5m/1h 都为空，unknown 计数单独返回。
pub fn attribute_cache_write(
    ttl: CacheWriteTtl,
    cache_creation_tokens: i64,
) -> (Option<i64>, Option<i64>, i64) {
    if cache_creation_tokens <= 0 {
        return (None, None, 0);
    }
    match ttl {
        CacheWriteTtl::FiveMin => (Some(cache_creation_tokens), None, 0),
        CacheWriteTtl::OneHour => (None, Some(cache_creation_tokens), 0),
        CacheWriteTtl::Unknown => (None, None, cache_creation_tokens),
    }
}

pub fn parse_cost_assumptions(raw: &str) -> CostAssumptions {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "[]" || trimmed == "null" {
        return CostAssumptions::default();
    }
    match serde_json::from_str::<CostAssumptions>(trimmed) {
        Ok(mut doc) => {
            if doc.schema_version == 0 {
                doc.schema_version = COST_SCHEMA_VERSION;
            }
            doc
        }
        Err(_) => {
            let mut doc = CostAssumptions::default();
            push_assumption(&mut doc, ASSUMPTION_PARSE_FAILED);
            doc
        }
    }
}

pub fn serialize_cost_assumptions(doc: &CostAssumptions) -> String {
    let mut current = doc.clone();
    let mut encoded = serde_json::to_string(&current).unwrap_or_else(|_| "{}".into());
    if encoded.len() <= COST_ASSUMPTIONS_MAX_LEN {
        return encoded;
    }
    current.candidate = None;
    encoded = serde_json::to_string(&current).unwrap_or_else(|_| "{}".into());
    while encoded.len() > COST_ASSUMPTIONS_MAX_LEN && current.assumptions.len() > 1 {
        current.assumptions.truncate(current.assumptions.len() / 2);
        push_assumption(&mut current, ASSUMPTION_TRUNCATED);
        encoded = serde_json::to_string(&current).unwrap_or_else(|_| "{}".into());
    }
    if encoded.len() > COST_ASSUMPTIONS_MAX_LEN {
        let mut fallback = CostAssumptions::default();
        push_assumption(&mut fallback, ASSUMPTION_TRUNCATED);
        fallback.cache_write_ttl = current.cache_write_ttl.clone();
        return serde_json::to_string(&fallback).unwrap_or_else(|_| "{}".into());
    }
    encoded
}

/// Create 时写入的 TTL 假设（尚无价格快照）。
pub fn initial_cost_assumptions_json(body: &Value) -> String {
    let ttl = classify_cache_write_ttl(body);
    let mut doc = CostAssumptions {
        cache_write_ttl: ttl.as_str().to_string(),
        breakdown_coverage: if ttl == CacheWriteTtl::Unknown {
            "partial".into()
        } else {
            "full".into()
        },
        ..CostAssumptions::default()
    };
    push_assumption(&mut doc, ttl.assumption());
    stamp_model_versions(&mut doc);
    serialize_cost_assumptions(&doc)
}

pub fn decimal_to_store(value: Decimal) -> String {
    value.normalize().to_string()
}

fn parse_decimal(raw: &str) -> Option<Decimal> {
    Decimal::from_str(raw.trim()).ok()
}

fn line_cost(tokens: i64, per_million: Decimal) -> Decimal {
    Decimal::from(tokens.max(0)) * per_million / Decimal::from(MILLION)
}

fn billable_input(tokens: TokenCounts, input_includes_cache: bool) -> i64 {
    if input_includes_cache {
        tokens
            .input
            .saturating_sub(tokens.cache_read.max(0))
            .saturating_sub(tokens.cache_creation.max(0))
    } else {
        tokens.input.max(0)
    }
}

/// 基座已验证口径：input/output/read + 按 TTL 拆分的 write（unknown 用合并 write 价）。
pub fn compute_actual_cost(
    tokens: TokenCounts,
    ttl: CacheWriteTtl,
    rates: &PriceRates,
    input_includes_cache: bool,
) -> Decimal {
    let (write_5m, write_1h, write_unknown) = attribute_cache_write(ttl, tokens.cache_creation);
    let input = billable_input(tokens, input_includes_cache);
    let mut total = line_cost(input, rates.input)
        + line_cost(tokens.output, rates.output)
        + line_cost(tokens.cache_read, rates.cache_read)
        + line_cost(write_5m.unwrap_or(0), rates.cache_write_5m)
        + line_cost(write_1h.unwrap_or(0), rates.cache_write_1h)
        + line_cost(write_unknown, rates.write_for_ttl(ttl));

    let replay = line_cost(input, rates.input) + line_cost(tokens.output, rates.output);
    total += replay * Decimal::from(tokens.retry_count.max(0));
    total += replay * Decimal::from(tokens.fallback_count.max(0));
    total
}

/// Low：同输出、缓存命中保留、无重试/Failover。
/// Base：尚无历史中位数时等于 Low，并打标。
/// High：Cache Bust + 更多输出 + 一次重试。
pub fn compute_candidate_range(
    tokens: TokenCounts,
    ttl: CacheWriteTtl,
    rates: &PriceRates,
    input_includes_cache: bool,
) -> (Decimal, Decimal, Decimal) {
    let core = TokenCounts {
        retry_count: 0,
        fallback_count: 0,
        ..tokens
    };
    let low = compute_actual_cost(core, ttl, rates, input_includes_cache);
    let base = low;

    let input = billable_input(tokens, input_includes_cache);
    let write_price = rates.write_for_ttl(ttl);
    let bust =
        line_cost(tokens.cache_read, write_price) - line_cost(tokens.cache_read, rates.cache_read);
    let extra_output = line_cost(tokens.output, rates.output)
        * Decimal::from_str(HIGH_OUTPUT_UPLIFT).unwrap_or(Decimal::ZERO);
    let one_retry = line_cost(input, rates.input) + line_cost(tokens.output, rates.output);
    let high = low + bust.max(Decimal::ZERO) + extra_output + one_retry;
    (low, base, high)
}

pub fn evaluate_costs(
    tokens: TokenCounts,
    ttl: CacheWriteTtl,
    baseline: Option<&PriceRates>,
    candidate_rates: Option<&PriceRates>,
    input_includes_cache: bool,
    mut assumptions: CostAssumptions,
) -> CostOutcome {
    let (write_5m, write_1h, write_unknown) = attribute_cache_write(ttl, tokens.cache_creation);
    assumptions.cache_write_ttl = ttl.as_str().to_string();
    push_assumption(&mut assumptions, ttl.assumption());
    if write_unknown > 0 {
        assumptions.breakdown_coverage = "partial".into();
    } else {
        assumptions.breakdown_coverage = "full".into();
    }

    let Some(actual_rates) = baseline else {
        push_assumption(&mut assumptions, ASSUMPTION_PRICE_MISSING);
        return CostOutcome {
            actual_usd: None,
            candidate: CandidateRange {
                low_usd: None,
                base_usd: None,
                high_usd: None,
            },
            write_5m,
            write_1h,
            assumptions,
        };
    };

    push_assumption(&mut assumptions, ASSUMPTION_CACHE_HIT_PRESERVED);
    push_assumption(&mut assumptions, ASSUMPTION_RETRY_REPLAY);
    push_assumption(&mut assumptions, ASSUMPTION_SHADOW_NO_SAVING);
    push_assumption(&mut assumptions, ASSUMPTION_WRITE_PRICE_COMBINED);

    let actual = compute_actual_cost(tokens, ttl, actual_rates, input_includes_cache);
    let proj_rates = if let Some(rates) = candidate_rates {
        rates
    } else {
        push_assumption(&mut assumptions, ASSUMPTION_CANDIDATE_USED_BASELINE_PRICES);
        actual_rates
    };
    push_assumption(&mut assumptions, ASSUMPTION_HISTORICAL_MEDIAN_UNAVAILABLE);
    push_assumption(&mut assumptions, ASSUMPTION_CACHE_BUST_HIGH);
    let (low, base, high) = compute_candidate_range(tokens, ttl, proj_rates, input_includes_cache);

    CostOutcome {
        actual_usd: Some(decimal_to_store(actual)),
        candidate: CandidateRange {
            low_usd: Some(decimal_to_store(low)),
            base_usd: Some(decimal_to_store(base)),
            high_usd: Some(decimal_to_store(high)),
        },
        write_5m,
        write_1h,
        assumptions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sonnet_rates() -> PriceRates {
        PriceRates {
            input: Decimal::from_str("3.0").unwrap(),
            output: Decimal::from_str("15.0").unwrap(),
            cache_read: Decimal::from_str("0.3").unwrap(),
            cache_write_5m: Decimal::from_str("3.75").unwrap(),
            cache_write_1h: Decimal::from_str("6.00").unwrap(),
        }
    }

    fn sample_tokens() -> TokenCounts {
        TokenCounts {
            input: 1000,
            output: 500,
            cache_read: 200,
            cache_creation: 100,
            retry_count: 0,
            fallback_count: 0,
        }
    }

    #[test]
    fn amount_precision_matches_decimal_formula() {
        let rates = sonnet_rates();
        let tokens = sample_tokens();
        let cost = compute_actual_cost(tokens, CacheWriteTtl::FiveMin, &rates, false);
        // 1000*3/1M + 500*15/1M + 200*0.3/1M + 100*3.75/1M
        let expected = Decimal::from_str("0.003").unwrap()
            + Decimal::from_str("0.0075").unwrap()
            + Decimal::from_str("0.00006").unwrap()
            + Decimal::from_str("0.000375").unwrap();
        assert_eq!(cost, expected);
        assert_eq!(decimal_to_store(cost), expected.normalize().to_string());
        assert!(decimal_to_store(cost).contains('.'));
    }

    #[test]
    fn ttl_5m_1h_unknown_are_not_confused() {
        let body_5m = json!({
            "system": [{"type": "text", "text": "s", "cache_control": {"type": "ephemeral"}}]
        });
        let body_1h = json!({
            "system": [{"type": "text", "text": "s", "cache_control": {"type": "ephemeral", "ttl": "1h"}}]
        });
        let body_mixed = json!({
            "system": [
                {"type": "text", "text": "a", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "b", "cache_control": {"type": "ephemeral", "ttl": "1h"}}
            ]
        });
        let body_none = json!({"messages": [{"role": "user", "content": "hi"}]});

        assert_eq!(classify_cache_write_ttl(&body_5m), CacheWriteTtl::FiveMin);
        assert_eq!(classify_cache_write_ttl(&body_1h), CacheWriteTtl::OneHour);
        assert_eq!(
            classify_cache_write_ttl(&body_mixed),
            CacheWriteTtl::Unknown
        );
        assert_eq!(classify_cache_write_ttl(&body_none), CacheWriteTtl::Unknown);

        let (a5, a1, au) = attribute_cache_write(CacheWriteTtl::FiveMin, 100);
        assert_eq!(a5, Some(100));
        assert_eq!(a1, None);
        assert_eq!(au, 0);

        let (b5, b1, bu) = attribute_cache_write(CacheWriteTtl::OneHour, 100);
        assert_eq!(b5, None);
        assert_eq!(b1, Some(100));
        assert_eq!(bu, 0);

        let (c5, c1, cu) = attribute_cache_write(CacheWriteTtl::Unknown, 100);
        assert_eq!(c5, None);
        assert_eq!(c1, None);
        assert_eq!(cu, 100);
    }

    #[test]
    fn unknown_ttl_does_not_dump_tokens_into_5m_or_1h() {
        let outcome = evaluate_costs(
            sample_tokens(),
            CacheWriteTtl::Unknown,
            Some(&sonnet_rates()),
            None,
            false,
            CostAssumptions::default(),
        );
        assert_eq!(outcome.write_5m, None);
        assert_eq!(outcome.write_1h, None);
        assert_eq!(outcome.assumptions.cache_write_ttl, "unknown");
        assert_eq!(outcome.assumptions.breakdown_coverage, "partial");
        assert!(outcome
            .assumptions
            .assumptions
            .contains(&ASSUMPTION_TTL_UNKNOWN.to_string()));
        // 实际金额仍按基座合并 write 价计入，不得编成 0。
        assert!(outcome.actual_usd.is_some());
        let actual = Decimal::from_str(outcome.actual_usd.as_deref().unwrap()).unwrap();
        assert!(actual > Decimal::ZERO);
    }

    #[test]
    fn cache_bust_high_exceeds_low_and_base() {
        let (low, base, high) = compute_candidate_range(
            sample_tokens(),
            CacheWriteTtl::FiveMin,
            &sonnet_rates(),
            false,
        );
        assert_eq!(low, base);
        assert!(high > base);
        assert!(high > low);
        // Bust 差值 = cache_read * (write - read) / 1M = 200 * 3.45 / 1M
        let bust = Decimal::from_str("0.00069").unwrap();
        assert!(high - low >= bust);
    }

    #[test]
    fn candidate_cost_is_not_actual_saving() {
        let rates = sonnet_rates();
        let tokens = sample_tokens();
        let actual = compute_actual_cost(tokens, CacheWriteTtl::FiveMin, &rates, false);
        let (low, base, high) =
            compute_candidate_range(tokens, CacheWriteTtl::FiveMin, &rates, false);
        assert_eq!(actual, low);
        assert_ne!(high, actual);
        // Shadow 无真实节省：不得把 (actual - candidate) 当成 actual。
        let fake_saving = actual - base;
        assert_eq!(fake_saving, Decimal::ZERO);
        assert_ne!(actual, fake_saving);
        assert_ne!(high, fake_saving);
    }

    #[test]
    fn historical_price_update_does_not_change_frozen_snapshot_cost() {
        let frozen = CostAssumptions {
            baseline: Some(PriceLeg {
                provider_id: Some("p".into()),
                model_id: "claude-sonnet-4-20250514".into(),
                price_source: "builtin".into(),
                price_observed_at: 1,
                input_per_million: "3.0".into(),
                output_per_million: "15.0".into(),
                cache_read_per_million: "0.3".into(),
                cache_write_5m_per_million: "3.75".into(),
                cache_write_1h_per_million: "3.75".into(),
            }),
            ..CostAssumptions::default()
        };
        assert!(price_leg_is_frozen(frozen.baseline.as_ref()));

        let old_rates = frozen.baseline.as_ref().unwrap().rates().unwrap();
        let live_rates = PriceRates {
            input: Decimal::from_str("99").unwrap(),
            ..old_rates
        };
        let tokens = sample_tokens();
        let from_snapshot = compute_actual_cost(tokens, CacheWriteTtl::FiveMin, &old_rates, false);
        let from_live = compute_actual_cost(tokens, CacheWriteTtl::FiveMin, &live_rates, false);
        assert_ne!(from_snapshot, from_live);
        // 历史行必须继续用快照，而不是 live 价。
        let outcome = evaluate_costs(
            tokens,
            CacheWriteTtl::FiveMin,
            Some(&old_rates),
            None,
            false,
            frozen.clone(),
        );
        assert_eq!(
            outcome.actual_usd.as_deref(),
            Some(decimal_to_store(from_snapshot).as_str())
        );
    }

    #[test]
    fn missing_price_does_not_invent_amount() {
        let outcome = evaluate_costs(
            sample_tokens(),
            CacheWriteTtl::FiveMin,
            None,
            None,
            false,
            CostAssumptions::default(),
        );
        assert_eq!(outcome.actual_usd, None);
        assert_eq!(outcome.candidate.low_usd, None);
        assert!(outcome
            .assumptions
            .assumptions
            .contains(&ASSUMPTION_PRICE_MISSING.to_string()));
    }

    #[test]
    fn parse_failure_and_max_length_are_safe() {
        let failed = parse_cost_assumptions("{not-json");
        assert!(failed
            .assumptions
            .contains(&ASSUMPTION_PARSE_FAILED.to_string()));
        assert_eq!(parse_cost_assumptions("[]").baseline, None);

        let mut huge = CostAssumptions::default();
        for i in 0..2000 {
            huge.assumptions
                .push(format!("ASSUMPTION_{i:04}_{}", "x".repeat(20)));
        }
        let encoded = serialize_cost_assumptions(&huge);
        assert!(encoded.len() <= COST_ASSUMPTIONS_MAX_LEN);
        let parsed = parse_cost_assumptions(&encoded);
        assert!(parsed
            .assumptions
            .contains(&ASSUMPTION_TRUNCATED.to_string()));
    }

    #[test]
    fn initial_assumptions_json_records_ttl() {
        let body = json!({
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "hi", "cache_control": {"type": "ephemeral", "ttl": "1h"}}
            ]}]
        });
        let raw = initial_cost_assumptions_json(&body);
        let doc = parse_cost_assumptions(&raw);
        assert_eq!(doc.cache_write_ttl, "1h");
        assert_eq!(doc.breakdown_coverage, "full");
        assert!(doc.assumptions.contains(&ASSUMPTION_TTL_1H.to_string()));
        assert!(doc.baseline.is_none());
        assert_eq!(doc.capability_table_version, CAPABILITY_TABLE_VERSION);
        assert_eq!(doc.cost_model_version, COST_MODEL_VERSION);
        assert_eq!(doc.cache_stats_version, CACHE_STATS_VERSION);
    }

    #[test]
    fn stamped_versions_are_not_overwritten() {
        let mut doc = CostAssumptions {
            capability_table_version: "capability-table-v0.0".into(),
            cost_model_version: "cost-model-v0.0".into(),
            cache_stats_version: "cache-stats-v0.0".into(),
            ..CostAssumptions::default()
        };
        stamp_model_versions(&mut doc);
        assert_eq!(doc.capability_table_version, "capability-table-v0.0");
        assert_eq!(doc.cost_model_version, "cost-model-v0.0");
        assert_eq!(doc.cache_stats_version, "cache-stats-v0.0");
    }
}

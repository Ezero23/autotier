//! shadow_eval — 离线 Shadow 评测 CLI（不改任何真实请求）。
//!
//! 输入：stdin，每行一个 OpenAI 风格请求体 JSON：{"model": "...", "messages": [...]}
//! 输出：stdout，每行 {"slot": "...", "score": 0.0, "confidence": 0.0, "reasons": [...], ...}
//!
//! 用 AutoTier 自己的 extract_features + shadow_decide（纯函数），
//! 与 Potluck/8800 路由器的实际选择离线对比。

use std::io::{self, BufRead, Write};

use autotier_lib::{
    extract_features, shadow_decide, AppType, DecisionId, DecisionInput, RoutingMode,
    RoutingSessionState, FEATURE_VERSION,
};
use serde_json::{json, Value};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (i, line) in stdin.lock().lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let body: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"error": format!("parse: {e}")}));
                continue;
            }
        };

        let features = extract_features(&body, AppType::Claude, "shadow-eval");
        let input = DecisionInput {
            decision_id: DecisionId(format!("shadow-eval-{i}")),
            app_type: AppType::Claude,
            client_requested_model: body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            initial_selected_provider: None,
            features,
            session_state: RoutingSessionState::default(),
            mode: RoutingMode::Shadow,
            feature_version: FEATURE_VERSION.to_string(),
        };
        let result = shadow_decide(&input, 0);
        let f = &input.features;
        let _ = writeln!(
            out,
            "{}",
            json!({
                "slot": result.recommended_slot.map(|s| s.as_str()),
                "score": result.complexity_score,
                "confidence": result.confidence,
                "reasons": result.reason_codes.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>(),
                "safe_to_execute": result.safe_to_execute,
                "weighted_len": f.user_message_weighted_length,
                "constraints": f.constraint_count,
                "code_score": f.code_structure_score,
                "reasoning_kw": f.reasoning_keyword_count,
                "error_sig": f.error_signal_count,
                "unfenced_code": f.unfenced_code_hits,
                "context_bucket": format!("{:?}", f.context_token_bucket),
                "message_bucket": format!("{:?}", f.message_count_bucket),
            })
        );
    }
}

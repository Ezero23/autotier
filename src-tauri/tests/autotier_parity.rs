//! Phase 4D：Off/Shadow 抓包 Parity
//!
//! 在同一套五场景下对比：
//! - 上游请求 Method / Path / Header / Body
//! - 客户端响应与 SSE 事件序列
//! - 基座 `proxy_request_logs`
//!
//! 动态字段只允许出现在 `fixtures/parity_diff.rs` 的批准白名单中。
//! 仓库内 Off 即「无 AutoTier 效果」基线（4C 已证明 Off 全旁路）；
//! 另跑一遍 Off 自洽，确认基线可复现。

#[path = "fixtures/parity_diff.rs"]
mod parity_diff;
#[path = "support.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use autotier_lib::{AutotierRoutingConfigDto, Database, Provider};
use parity_diff::{
    diff_headers, diff_json, diff_json_parity, diff_str, format_report, Diff,
    WHITELIST_CLIENT_HEADERS, WHITELIST_UPSTREAM_HEADERS, WHITELIST_USAGE_FIELDS,
};
use serde_json::{json, Value};
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const MODEL: &str = "claude-sonnet-4-20250514";

// ============================================================================
// 抓包结构
// ============================================================================

#[derive(Debug, Clone)]
struct WireRequest {
    method: String,
    path: String,
    query: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

#[derive(Debug, Clone)]
struct ClientCapture {
    status: u16,
    headers: BTreeMap<String, String>,
    body_text: String,
}

#[derive(Debug, Clone)]
struct UsageSnap {
    provider_id: String,
    model: String,
    request_model: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    total_cost_usd: String,
    status_code: i64,
    is_streaming: i64,
    error_message: Option<String>,
}

#[derive(Debug, Clone)]
struct ScenarioSnap {
    name: &'static str,
    upstream: Vec<WireRequest>,
    client: ClientCapture,
    usage: UsageSnap,
}

// ============================================================================
// Mock 上游
// ============================================================================

struct MockUpstream {
    name: String,
    requests: Mutex<Vec<WireRequest>>,
    counter: AtomicUsize,
    always_error: bool,
}

impl MockUpstream {
    fn next_message_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        format!("msg_{}_{:04}", self.name, n)
    }

    fn snapshot(&self) -> Vec<WireRequest> {
        self.requests.lock().unwrap().clone()
    }
}

fn error_body() -> Value {
    json!({
        "type": "error",
        "error": {
            "type": "internal_error",
            "message": "mock upstream forced 500"
        }
    })
}

fn build_non_stream_response(message_id: &str, model: &str, with_tool: bool) -> Value {
    let content = if with_tool {
        json!([{
            "type": "tool_use",
            "id": "toolu_mock_01",
            "name": "get_weather",
            "input": {"city": "Shanghai"}
        }])
    } else {
        json!([{"type": "text", "text": "mock non-stream reply"}])
    };
    json!({
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": if with_tool { "tool_use" } else { "end_turn" },
        "stop_sequence": null,
        "usage": {
            "input_tokens": 15,
            "output_tokens": 8,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }
    })
}

fn build_sse_stream(message_id: &str, model: &str) -> String {
    let events = [
        (
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": message_id,
                    "type": "message",
                    "role": "assistant",
                    "model": model,
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {"input_tokens": 15, "output_tokens": 1}
                }
            }),
        ),
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "mock stream reply"}
            }),
        ),
        (
            "content_block_stop",
            json!({
                "type": "content_block_stop",
                "index": 0
            }),
        ),
        (
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                "usage": {"output_tokens": 9}
            }),
        ),
        ("message_stop", json!({"type": "message_stop"})),
    ];
    let mut out = String::new();
    for (event, data) in events {
        out.push_str(&format!("event: {event}\ndata: {data}\n\n"));
    }
    out
}

fn header_map(headers: &HeaderMap) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (name, value) in headers.iter() {
        let key = name.as_str().to_ascii_lowercase();
        if let Ok(v) = value.to_str() {
            map.entry(key)
                .and_modify(|existing: &mut String| {
                    existing.push(',');
                    existing.push_str(v);
                })
                .or_insert_with(|| v.to_string());
        }
    }
    map
}

async fn mock_messages(
    State(mock): State<Arc<MockUpstream>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> Response {
    let parsed: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
    mock.requests.lock().unwrap().push(WireRequest {
        method: method.as_str().to_string(),
        path: uri.path().to_string(),
        query: uri.query().unwrap_or("").to_string(),
        headers: header_map(&headers),
        body: parsed.clone(),
    });

    let model = parsed
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let is_stream = parsed
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_tools = parsed.get("tools").is_some();
    let force_error = mock.always_error || model.contains("force-500");

    if force_error {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "application/json")
            .body(error_body().to_string().into())
            .unwrap();
    }

    let message_id = mock.next_message_id();
    if is_stream {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .body(build_sse_stream(&message_id, &model).into())
            .unwrap()
    } else {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(
                build_non_stream_response(&message_id, &model, has_tools)
                    .to_string()
                    .into(),
            )
            .unwrap()
    }
}

async fn start_mock_upstream(name: &str, always_error: bool) -> (u16, Arc<MockUpstream>) {
    let mock = Arc::new(MockUpstream {
        name: name.to_string(),
        requests: Mutex::new(Vec::new()),
        counter: AtomicUsize::new(0),
        always_error,
    });
    let app = Router::new()
        .route("/v1/messages", post(mock_messages))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let port = listener.local_addr().expect("addr").port();
    let task_name = name.to_string();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[mock:{task_name}] {e}");
        }
    });
    (port, mock)
}

// ============================================================================
// Usage dump
// ============================================================================

fn dump_usage(db_path: &Path) -> Vec<UsageSnap> {
    let conn = rusqlite::Connection::open(db_path).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT provider_id, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, status_code, is_streaming, error_message
             FROM proxy_request_logs ORDER BY created_at, rowid",
        )
        .expect("prepare");
    stmt.query_map([], |row| {
        Ok(UsageSnap {
            provider_id: row.get(0)?,
            model: row.get(1)?,
            request_model: row.get(2)?,
            input_tokens: row.get(3)?,
            output_tokens: row.get(4)?,
            cache_read_tokens: row.get(5)?,
            cache_creation_tokens: row.get(6)?,
            total_cost_usd: row.get(7)?,
            status_code: row.get(8)?,
            is_streaming: row.get(9)?,
            error_message: row.get(10)?,
        })
    })
    .expect("query")
    .collect::<Result<Vec<_>, _>>()
    .expect("collect")
}

async fn wait_for_log_rows(db_path: &Path, expected: usize, timeout: Duration) -> Vec<UsageSnap> {
    let start = Instant::now();
    loop {
        let rows = dump_usage(db_path);
        if rows.len() >= expected {
            return rows;
        }
        if start.elapsed() > timeout {
            panic!("等待 usage 超时: 期望 {expected} 行，实际 {}", rows.len());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn db_path() -> std::path::PathBuf {
    ensure_test_home().join(".autotier").join("autotier.db")
}

fn set_autotier_mode(db: &Database, mode: &str) {
    db.autotier_set_config(&AutotierRoutingConfigDto {
        mode: mode.into(),
        ..Default::default()
    })
    .expect("set mode");
}

fn make_claude_provider(id: &str, name: &str, port: u16) -> Provider {
    Provider::with_id(
        id.to_string(),
        name.to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": format!("http://127.0.0.1:{port}"),
                "ANTHROPIC_AUTH_TOKEN": "mock-token"
            }
        }),
        None,
    )
}

fn request_body(model: &str, stream: bool) -> Value {
    json!({
        "model": model,
        "max_tokens": 64,
        "stream": stream,
        "messages": [{"role": "user", "content": "say hi"}]
    })
}

fn tools_body() -> Value {
    let mut body = request_body(MODEL, false);
    body["tools"] = json!([{
        "name": "get_weather",
        "description": "Get weather for a city",
        "input_schema": {
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        }
    }]);
    body
}

fn sse_events(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.strip_prefix("event: "))
        .collect()
}

async fn capture_client(resp: reqwest::Response) -> ClientCapture {
    let status = resp.status().as_u16();
    let mut headers = BTreeMap::new();
    for (name, value) in resp.headers().iter() {
        let key = name.as_str().to_ascii_lowercase();
        if let Ok(v) = value.to_str() {
            headers
                .entry(key)
                .and_modify(|existing: &mut String| {
                    existing.push(',');
                    existing.push_str(v);
                })
                .or_insert_with(|| v.to_string());
        }
    }
    let body_text = resp.text().await.expect("read body");
    ClientCapture {
        status,
        headers,
        body_text,
    }
}

fn assert_no_autotier_fingerprint(reqs: &[WireRequest]) {
    for req in reqs {
        for key in req.headers.keys() {
            assert!(
                !key.contains("autotier"),
                "上游请求不得带 AutoTier 头: {key}"
            );
        }
        let blob = req.body.to_string().to_ascii_lowercase();
        assert!(!blob.contains("autotier"), "上游 body 不得含 autotier 字段");
    }
}

fn diff_usage(prefix: &str, left: &UsageSnap, right: &UsageSnap) -> Vec<Diff> {
    let l_in = left.input_tokens.to_string();
    let r_in = right.input_tokens.to_string();
    let l_out = left.output_tokens.to_string();
    let r_out = right.output_tokens.to_string();
    let l_cr = left.cache_read_tokens.to_string();
    let r_cr = right.cache_read_tokens.to_string();
    let l_cc = left.cache_creation_tokens.to_string();
    let r_cc = right.cache_creation_tokens.to_string();
    let l_st = left.status_code.to_string();
    let r_st = right.status_code.to_string();
    let l_sm = left.is_streaming.to_string();
    let r_sm = right.is_streaming.to_string();
    let pairs = [
        (
            "provider_id",
            left.provider_id.as_str(),
            right.provider_id.as_str(),
        ),
        ("model", left.model.as_str(), right.model.as_str()),
        (
            "request_model",
            left.request_model.as_deref().unwrap_or(""),
            right.request_model.as_deref().unwrap_or(""),
        ),
        ("input_tokens", l_in.as_str(), r_in.as_str()),
        ("output_tokens", l_out.as_str(), r_out.as_str()),
        ("cache_read_tokens", l_cr.as_str(), r_cr.as_str()),
        ("cache_creation_tokens", l_cc.as_str(), r_cc.as_str()),
        (
            "total_cost_usd",
            left.total_cost_usd.as_str(),
            right.total_cost_usd.as_str(),
        ),
        ("status_code", l_st.as_str(), r_st.as_str()),
        ("is_streaming", l_sm.as_str(), r_sm.as_str()),
        (
            "error_message",
            left.error_message.as_deref().unwrap_or(""),
            right.error_message.as_deref().unwrap_or(""),
        ),
    ];
    let mut diffs = Vec::new();
    for (field, l, r) in pairs {
        let path = format!("{prefix}.{field}");
        diffs.extend(diff_str(&path, l, r, WHITELIST_USAGE_FIELDS));
    }
    diffs
}

fn diff_client_body(left: &ClientCapture, right: &ClientCapture) -> Vec<Diff> {
    let l_json = serde_json::from_str::<Value>(&left.body_text).ok();
    let r_json = serde_json::from_str::<Value>(&right.body_text).ok();
    match (l_json, r_json) {
        (Some(a), Some(b)) => diff_json_parity("client.body", &a, &b),
        _ => {
            let mut diffs = diff_str("client.body", &left.body_text, &right.body_text, &[]);
            let le = sse_events(&left.body_text);
            let re = sse_events(&right.body_text);
            if le != re {
                diffs.push(Diff::new(
                    "client.sse_events",
                    format!("{le:?}"),
                    format!("{re:?}"),
                ));
            }
            diffs
        }
    }
}

fn compare_snaps(
    left_label: &str,
    right_label: &str,
    left: &[ScenarioSnap],
    right: &[ScenarioSnap],
) {
    assert_eq!(
        left.len(),
        right.len(),
        "{left_label} vs {right_label} 场景数不同"
    );
    let mut all = Vec::new();
    for (a, b) in left.iter().zip(right.iter()) {
        let prefix = a.name;
        all.extend(diff_str(&format!("{prefix}.name"), a.name, b.name, &[]));
        if a.upstream.len() != b.upstream.len() {
            all.push(Diff::new(
                format!("{prefix}.upstream.len"),
                a.upstream.len().to_string(),
                b.upstream.len().to_string(),
            ));
            continue;
        }
        for (i, (u, v)) in a.upstream.iter().zip(b.upstream.iter()).enumerate() {
            let p = format!("{prefix}.upstream[{i}]");
            all.extend(diff_str(&format!("{p}.method"), &u.method, &v.method, &[]));
            all.extend(diff_str(&format!("{p}.path"), &u.path, &v.path, &[]));
            all.extend(diff_str(&format!("{p}.query"), &u.query, &v.query, &[]));
            all.extend(diff_headers(
                &format!("{p}.headers"),
                &u.headers,
                &v.headers,
                WHITELIST_UPSTREAM_HEADERS,
            ));
            all.extend(diff_json(&format!("{p}.body"), &u.body, &v.body));
            assert_eq!(
                u.body.get("model"),
                v.body.get("model"),
                "{prefix} AutoTier 不得改变上游 model"
            );
        }
        all.extend(diff_str(
            &format!("{prefix}.client.status"),
            &a.client.status.to_string(),
            &b.client.status.to_string(),
            &[],
        ));
        all.extend(diff_headers(
            &format!("{prefix}.client.headers"),
            &a.client.headers,
            &b.client.headers,
            WHITELIST_CLIENT_HEADERS,
        ));
        all.extend(diff_client_body(&a.client, &b.client));
        all.extend(diff_usage(&format!("{prefix}.usage"), &a.usage, &b.usage));
        assert_eq!(
            a.usage.provider_id, b.usage.provider_id,
            "{prefix} AutoTier 不得改变 Usage provider"
        );
        assert_eq!(
            a.usage.model, b.usage.model,
            "{prefix} AutoTier 不得改变 Usage model"
        );
    }
    if !all.is_empty() {
        panic!(
            "{}",
            format_report(&format!("{left_label} vs {right_label}"), &all)
        );
    }
}

// ============================================================================
// 跑一套五场景
// ============================================================================

async fn send(
    client: &reqwest::Client,
    port: u16,
    body: &Value,
    session: &str,
) -> reqwest::Response {
    client
        .post(format!("http://127.0.0.1:{port}/v1/messages"))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-claude-code-session-id", session)
        .json(body)
        .send()
        .await
        .expect("send")
}

fn take_since(mock: &MockUpstream, since: usize) -> Vec<WireRequest> {
    mock.snapshot().into_iter().skip(since).collect()
}

async fn run_five_scenarios(mode: &str) -> (Vec<ScenarioSnap>, i64) {
    reset_test_fs();
    let state = create_test_state().expect("state");
    let db = state.db.clone();
    set_autotier_mode(&db, mode);

    let (good_port, good) = start_mock_upstream("good", false).await;
    let (backup_port, backup) = start_mock_upstream("backup", false).await;
    let (fail_port, fail) = start_mock_upstream("fail", true).await;

    let p1 = make_claude_provider("p1", "Mock Primary", good_port);
    db.save_provider("claude", &p1).expect("save p1");
    db.set_current_provider("claude", "p1").expect("current p1");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("client");
    let logs = db_path();
    let mut snaps = Vec::new();

    // a. 非流式
    let before_good = good.snapshot().len();
    let client_a =
        capture_client(send(&client, port, &request_body(MODEL, false), "parity-a").await).await;
    let usage = wait_for_log_rows(&logs, 1, Duration::from_secs(3)).await;
    snaps.push(ScenarioSnap {
        name: "non_stream",
        upstream: take_since(&good, before_good),
        client: client_a,
        usage: usage.last().unwrap().clone(),
    });

    // b. SSE
    let before_good = good.snapshot().len();
    let client_b =
        capture_client(send(&client, port, &request_body(MODEL, true), "parity-b").await).await;
    let usage = wait_for_log_rows(&logs, 2, Duration::from_secs(3)).await;
    snaps.push(ScenarioSnap {
        name: "sse",
        upstream: take_since(&good, before_good),
        client: client_b,
        usage: usage.last().unwrap().clone(),
    });

    // c. Tool Use
    let before_good = good.snapshot().len();
    let client_c = capture_client(send(&client, port, &tools_body(), "parity-c").await).await;
    let usage = wait_for_log_rows(&logs, 3, Duration::from_secs(3)).await;
    snaps.push(ScenarioSnap {
        name: "tool_use",
        upstream: take_since(&good, before_good),
        client: client_c,
        usage: usage.last().unwrap().clone(),
    });

    // d. 500
    let before_good = good.snapshot().len();
    let client_d = capture_client(
        send(
            &client,
            port,
            &request_body("claude-force-500", false),
            "parity-d",
        )
        .await,
    )
    .await;
    let usage = wait_for_log_rows(&logs, 4, Duration::from_secs(3)).await;
    snaps.push(ScenarioSnap {
        name: "status_500",
        upstream: take_since(&good, before_good),
        client: client_d,
        usage: usage.last().unwrap().clone(),
    });

    // e. Failover
    let mut pfail = make_claude_provider("pfail", "Mock Failing", fail_port);
    pfail.sort_index = Some(1);
    let mut pbackup = make_claude_provider("pbackup", "Mock Backup", backup_port);
    pbackup.sort_index = Some(2);
    db.save_provider("claude", &pfail).expect("save pfail");
    db.save_provider("claude", &pbackup).expect("save pbackup");
    db.add_to_failover_queue("claude", "pfail")
        .expect("queue pfail");
    db.add_to_failover_queue("claude", "pbackup")
        .expect("queue pbackup");
    db.set_current_provider("claude", "pfail")
        .expect("current pfail");
    let mut app_cfg = db.get_proxy_config_for_app("claude").await.expect("cfg");
    app_cfg.auto_failover_enabled = true;
    db.update_proxy_config_for_app(app_cfg)
        .await
        .expect("enable failover");

    let before_fail = fail.snapshot().len();
    let before_backup = backup.snapshot().len();
    let client_e =
        capture_client(send(&client, port, &request_body(MODEL, false), "parity-e").await).await;
    let usage = wait_for_log_rows(&logs, 5, Duration::from_secs(3)).await;
    let mut upstream = take_since(&fail, before_fail);
    upstream.extend(take_since(&backup, before_backup));
    snaps.push(ScenarioSnap {
        name: "failover",
        upstream,
        client: client_e,
        usage: usage.last().unwrap().clone(),
    });

    for snap in &snaps {
        assert_no_autotier_fingerprint(&snap.upstream);
    }

    let decisions = db.autotier_count_decisions().expect("count decisions");
    state.proxy_service.stop().await.expect("stop");
    (snaps, decisions)
}

// ============================================================================
// 测试
// ============================================================================

#[test]
fn whitelist_does_not_cover_model_or_provider() {
    let diffs = diff_json(
        "body",
        &json!({"model": "a", "provider": "p1"}),
        &json!({"model": "b", "provider": "p1"}),
    );
    assert!(
        diffs.iter().any(|d| d.path == "body.model"),
        "model 差异不得被白名单吞掉: {diffs:?}"
    );
}

#[test]
fn host_header_is_whitelisted() {
    let mut left = BTreeMap::new();
    left.insert("host".into(), "127.0.0.1:1".into());
    left.insert("content-type".into(), "application/json".into());
    let mut right = BTreeMap::new();
    right.insert("host".into(), "127.0.0.1:2".into());
    right.insert("content-type".into(), "application/json".into());
    let diffs = diff_headers("headers", &left, &right, WHITELIST_UPSTREAM_HEADERS);
    assert!(diffs.is_empty(), "{diffs:?}");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn off_is_stable_baseline() {
    let _guard = test_mutex().lock().expect("mutex");
    let (first, n1) = run_five_scenarios("off").await;
    let (second, n2) = run_five_scenarios("off").await;
    assert_eq!(n1, 0, "Off 零 Decision");
    assert_eq!(n2, 0, "Off 零 Decision");
    compare_snaps("off-a", "off-b", &first, &second);
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn off_vs_shadow_parity_five_scenarios() {
    let _guard = test_mutex().lock().expect("mutex");
    let (off, off_n) = run_five_scenarios("off").await;
    let (shadow, shadow_n) = run_five_scenarios("shadow").await;
    assert_eq!(off_n, 0, "Off 零 Decision");
    assert_eq!(shadow_n, 5, "Shadow 五场景各一条 Decision");
    assert_eq!(off.len(), 5);
    assert_eq!(shadow.len(), 5);
    assert_eq!(off[0].client.status, 200);
    assert_eq!(off[1].client.status, 200);
    assert_eq!(off[2].client.status, 200);
    assert_eq!(off[3].client.status, 500);
    assert_eq!(off[4].client.status, 200);
    assert_eq!(off[4].usage.provider_id, "pbackup");
    compare_snaps("off", "shadow", &off, &shadow);
}

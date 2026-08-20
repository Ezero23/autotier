//! Phase 4C 集成测试：Claude Handler Shadow 接入
//!
//! 验证内容：
//! 1. 非流式 / SSE / Tool Use / 500 / Failover 五场景
//! 2. `autotier_mutated_request=false` 且 Actual = Baseline
//! 3. Initial Provider 与 Failover Actual Provider 正确区分
//! 4. Off 零决策；Shadow 不增加网络调用
//! 5. DB 失败不影响请求

#[path = "support.rs"]
mod support;

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use cc_switch_lib::{AutotierDecisionRow, AutotierRoutingConfigDto, Database, Provider};
use serde_json::{json, Value};
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

const MODEL: &str = "claude-sonnet-4-20250514";

// ============================================================================
// Mock 上游
// ============================================================================

struct MockUpstream {
    name: String,
    requests: Mutex<Vec<Value>>,
    counter: AtomicUsize,
    always_error: bool,
}

impl MockUpstream {
    fn next_message_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        format!("msg_{}_{:04}", self.name, n)
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
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
            "input_tokens": 10,
            "output_tokens": 5,
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
                    "usage": {"input_tokens": 10, "output_tokens": 1}
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

async fn mock_messages(State(mock): State<Arc<MockUpstream>>, body: String) -> Response {
    let parsed: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
    mock.requests.lock().unwrap().push(parsed.clone());

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
        .expect("bind mock upstream");
    let port = listener.local_addr().expect("mock local addr").port();
    let task_name = name.to_string();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[mock:{task_name}] serve error: {e}");
        }
    });
    (port, mock)
}

// ============================================================================
// 辅助函数
// ============================================================================

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

fn count_decisions(db: &Database) -> i64 {
    db.autotier_count_decisions().expect("count decisions")
}

fn list_recent_decisions(db: &Database) -> Vec<AutotierDecisionRow> {
    let now = chrono::Utc::now().timestamp_millis();
    db.autotier_list_decisions(0, now + 10000, 100, 0)
        .expect("list decisions")
}

async fn wait_for_completed(db: &Database, expected: usize, timeout: Duration) {
    let start = Instant::now();
    loop {
        let rows = list_recent_decisions(db);
        let n = rows.iter().filter(|r| r.completed_at.is_some()).count();
        if n >= expected {
            return;
        }
        if start.elapsed() > timeout {
            panic!(
                "等待 finalize 超时: 期望 completed {expected}, 实际 {n}, 行数 {}",
                rows.len()
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_for_usage_linked(db: &Database, expected: usize, timeout: Duration) {
    let start = Instant::now();
    loop {
        let rows = list_recent_decisions(db);
        let n = rows
            .iter()
            .filter(|r| r.usage_request_id.is_some() && r.is_complete)
            .count();
        if n >= expected {
            return;
        }
        if start.elapsed() > timeout {
            panic!(
                "等待 usage 关联超时: 期望 linked {expected}, 实际 {n}, 行数 {}",
                rows.len()
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn set_autotier_mode(db: &Database, mode: &str) {
    db.autotier_set_config(&AutotierRoutingConfigDto {
        mode: mode.into(),
        ..Default::default()
    })
    .expect("set autotier mode");
}

fn db_file_path() -> std::path::PathBuf {
    let home = ensure_test_home().to_path_buf();
    home.join(".autotier").join("autotier.db")
}

fn assert_shadow_unmutated(row: &AutotierDecisionRow) {
    assert_eq!(row.mode, "shadow");
    assert!(!row.autotier_mutated_request);
    assert_eq!(row.actual_outbound_model, row.baseline_outbound_model);
    assert_eq!(row.actual_outbound_provider, row.baseline_outbound_provider);
    if row.candidate_model.is_some() {
        assert_ne!(
            row.actual_outbound_model, row.candidate_model,
            "Candidate 不得进入 Actual"
        );
    }
}

fn assert_usage_linked(row: &AutotierDecisionRow, message_id: &str) {
    assert_eq!(row.upstream_message_id.as_deref(), Some(message_id));
    let expected = format!("session:{message_id}");
    assert_eq!(row.usage_request_id.as_deref(), Some(expected.as_str()));
    assert!(row.is_complete, "有 Usage 时应 is_complete");
    assert!(row.actual_input_tokens.unwrap_or(0) > 0);
}

async fn post_messages(port: u16, body: &Value) -> reqwest::Response {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build client");
    client
        .post(format!("http://127.0.0.1:{port}/v1/messages"))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(body)
        .send()
        .await
        .expect("send request")
}

// ============================================================================
// 测试
// ============================================================================

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shadow_non_streaming_preserves_model_and_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();
    set_autotier_mode(&db, "shadow");

    let (upstream_port, mock) = start_mock_upstream("good", false).await;
    let provider = make_claude_provider("p-shadow", "Mock Shadow", upstream_port);
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p-shadow")
        .expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let resp = post_messages(port, &request_body(MODEL, false)).await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("parse response");
    assert_eq!(body["model"], MODEL);
    assert_eq!(mock.request_count(), 1, "Shadow 不得增加网络调用");
    let message_id = body["id"].as_str().expect("message id").to_string();

    wait_for_usage_linked(&db, 1, Duration::from_secs(3)).await;
    let decisions = list_recent_decisions(&db);
    assert_eq!(decisions.len(), 1);
    let row = &decisions[0];
    assert_shadow_unmutated(row);
    assert_usage_linked(row, &message_id);
    assert_eq!(row.client_requested_model, MODEL);
    assert_eq!(row.baseline_outbound_model.as_deref(), Some(MODEL));
    assert_eq!(row.actual_outbound_model.as_deref(), Some(MODEL));
    assert_eq!(row.initial_selected_provider.as_deref(), Some("p-shadow"));
    assert_eq!(row.baseline_outbound_provider.as_deref(), Some("p-shadow"));
    assert_eq!(row.actual_outbound_provider.as_deref(), Some("p-shadow"));
    assert_eq!(row.status_code, Some(200));
    assert_eq!(row.outcome.as_deref(), Some("success"));
    assert_eq!(row.fallback_count, 0);

    state.proxy_service.stop().await.expect("stop proxy");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shadow_streaming_preserves_model_and_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();
    set_autotier_mode(&db, "shadow");

    let (upstream_port, mock) = start_mock_upstream("sse", false).await;
    let provider = make_claude_provider("p-shadow-stream", "Mock Shadow Stream", upstream_port);
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p-shadow-stream")
        .expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let resp = post_messages(port, &request_body(MODEL, true)).await;
    assert_eq!(resp.status(), 200);
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(content_type.contains("text/event-stream"), "应为 SSE");

    let text = resp.text().await.expect("read stream body");
    let events: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("event: "))
        .collect();
    assert!(events.contains(&"message_start"));
    assert!(events.contains(&"message_stop"));
    assert_eq!(mock.request_count(), 1, "Shadow 不得增加网络调用");
    let message_id = text
        .lines()
        .find_map(|line| {
            line.strip_prefix("data: ").and_then(|data| {
                serde_json::from_str::<Value>(data)
                    .ok()
                    .and_then(|v| v.pointer("/message/id")?.as_str().map(str::to_string))
            })
        })
        .expect("sse message id");

    wait_for_usage_linked(&db, 1, Duration::from_secs(3)).await;
    let decisions = list_recent_decisions(&db);
    assert_eq!(decisions.len(), 1);
    let row = &decisions[0];
    assert_shadow_unmutated(row);
    assert_usage_linked(row, &message_id);
    assert_eq!(row.client_requested_model, MODEL);
    assert_eq!(row.actual_outbound_model.as_deref(), Some(MODEL));
    assert_eq!(
        row.actual_outbound_provider.as_deref(),
        Some("p-shadow-stream")
    );
    assert_eq!(row.status_code, Some(200));

    state.proxy_service.stop().await.expect("stop proxy");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shadow_tool_use_preserves_model_and_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();
    set_autotier_mode(&db, "shadow");

    let (upstream_port, mock) = start_mock_upstream("tools", false).await;
    let provider = make_claude_provider("p-tools", "Mock Tools", upstream_port);
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p-tools")
        .expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

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

    let resp = post_messages(port, &body).await;
    assert_eq!(resp.status(), 200);
    let json_body: Value = resp.json().await.expect("parse response");
    assert_eq!(json_body["stop_reason"], "tool_use");
    assert_eq!(json_body["content"][0]["type"], "tool_use");
    assert_eq!(mock.request_count(), 1, "Shadow 不得增加网络调用");
    assert!(
        mock.requests.lock().unwrap()[0].get("tools").is_some(),
        "上游应收到 tools 字段"
    );
    let message_id = json_body["id"].as_str().expect("message id").to_string();

    wait_for_usage_linked(&db, 1, Duration::from_secs(3)).await;
    let row = &list_recent_decisions(&db)[0];
    assert_shadow_unmutated(row);
    assert_usage_linked(row, &message_id);
    assert_eq!(row.actual_outbound_model.as_deref(), Some(MODEL));
    assert_eq!(row.actual_outbound_provider.as_deref(), Some("p-tools"));
    assert_eq!(row.initial_selected_provider.as_deref(), Some("p-tools"));

    state.proxy_service.stop().await.expect("stop proxy");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shadow_upstream_500_writes_error_finalize() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();
    set_autotier_mode(&db, "shadow");

    let (upstream_port, mock) = start_mock_upstream("bad", true).await;
    let provider = make_claude_provider("p-500", "Mock 500", upstream_port);
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p-500")
        .expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let resp = post_messages(port, &request_body(MODEL, false)).await;
    assert_eq!(resp.status(), 500);
    assert_eq!(mock.request_count(), 1, "Shadow 不得增加网络调用");

    wait_for_completed(&db, 1, Duration::from_secs(3)).await;
    let row = &list_recent_decisions(&db)[0];
    assert_shadow_unmutated(row);
    assert_eq!(row.initial_selected_provider.as_deref(), Some("p-500"));
    assert_eq!(row.actual_outbound_provider.as_deref(), Some("p-500"));
    assert_eq!(row.baseline_outbound_provider.as_deref(), Some("p-500"));
    assert_eq!(row.status_code, Some(500));
    assert_eq!(row.outcome.as_deref(), Some("error"));
    assert_eq!(row.error_code.as_deref(), Some("upstream_500"));
    assert_eq!(row.fallback_count, 0);
    assert!(!row.is_complete, "无 Message ID 的 500 保持合法无 Usage");
    assert!(row.usage_request_id.is_none());
    assert!(row.upstream_message_id.is_none());

    state.proxy_service.stop().await.expect("stop proxy");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shadow_failover_distinguishes_initial_and_actual_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();
    set_autotier_mode(&db, "shadow");

    let (bad_port, bad_mock) = start_mock_upstream("fail", true).await;
    let (backup_port, backup_mock) = start_mock_upstream("backup", false).await;

    let mut pfail = make_claude_provider("pfail", "Mock Failing", bad_port);
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
        .expect("set current pfail");

    let mut app_cfg = db
        .get_proxy_config_for_app("claude")
        .await
        .expect("read app proxy config");
    app_cfg.auto_failover_enabled = true;
    db.update_proxy_config_for_app(app_cfg)
        .await
        .expect("enable auto failover");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let resp = post_messages(port, &request_body(MODEL, false)).await;
    assert_eq!(resp.status(), 200, "failover 后客户端应拿到 200");
    let json_e: Value = resp.json().await.expect("parse failover body");
    let message_id = json_e["id"].as_str().expect("message id").to_string();
    assert_eq!(bad_mock.request_count(), 1, "pfail 应被尝试 1 次");
    assert_eq!(
        backup_mock.request_count(),
        1,
        "pbackup 应接到 failover 后的请求；Shadow 不得再增加调用"
    );

    wait_for_usage_linked(&db, 1, Duration::from_secs(3)).await;
    let row = &list_recent_decisions(&db)[0];
    assert_shadow_unmutated(row);
    assert_usage_linked(row, &message_id);
    assert_eq!(
        row.initial_selected_provider.as_deref(),
        Some("pfail"),
        "首次选中应为故障转移链第一家"
    );
    assert_eq!(
        row.actual_outbound_provider.as_deref(),
        Some("pbackup"),
        "实际出站应为 failover 后的 backup"
    );
    assert_eq!(
        row.baseline_outbound_provider.as_deref(),
        Some("pbackup"),
        "Shadow 下 Baseline 也是基座 failover 后的真值"
    );
    assert_eq!(row.actual_outbound_model.as_deref(), Some(MODEL));
    assert_eq!(row.fallback_count, 1);
    assert_eq!(row.status_code, Some(200));
    assert_eq!(row.outcome.as_deref(), Some("success"));

    state.proxy_service.stop().await.expect("stop proxy");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn off_mode_does_not_write_decision() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();
    set_autotier_mode(&db, "off");

    let (upstream_port, mock) = start_mock_upstream("off", false).await;
    let provider = make_claude_provider("p-off", "Mock Off", upstream_port);
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p-off")
        .expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let resp = post_messages(port, &request_body(MODEL, false)).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(mock.request_count(), 1);

    tokio::time::sleep(Duration::from_millis(300)).await;
    let n = count_decisions(&db);
    assert_eq!(n, 0, "off 模式不应写入任何 decision");

    state.proxy_service.stop().await.expect("stop proxy");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shadow_db_failure_does_not_block_request() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();
    set_autotier_mode(&db, "shadow");

    let (upstream_port, _mock) = start_mock_upstream("db-fail", false).await;
    let provider = make_claude_provider("p-db-fail", "Mock DB Fail", upstream_port);
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p-db-fail")
        .expect("set current provider");

    let db_path = db_file_path();
    let mut perms = std::fs::metadata(&db_path)
        .expect("get db metadata")
        .permissions();
    perms.set_mode(0o444);
    std::fs::set_permissions(&db_path, perms).expect("set db read-only");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let resp = post_messages(port, &request_body(MODEL, false)).await;
    assert_eq!(resp.status(), 200, "DB 失败不应阻塞请求");
    let body: Value = resp.json().await.expect("parse response");
    assert_eq!(body["model"], MODEL);

    state.proxy_service.stop().await.expect("stop proxy");
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eligible_usage_link_rate_is_at_least_99_percent() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();
    set_autotier_mode(&db, "shadow");

    let (upstream_port, _mock) = start_mock_upstream("link", false).await;
    let provider = make_claude_provider("p-link", "Mock Link", upstream_port);
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p-link")
        .expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    const N: usize = 20;
    for i in 0..N {
        let resp = post_messages(port, &request_body(MODEL, false)).await;
        assert_eq!(resp.status(), 200, "request {i}");
        let _ = resp.bytes().await;
    }

    wait_for_usage_linked(&db, N, Duration::from_secs(5)).await;
    let rows = list_recent_decisions(&db);
    let eligible = rows
        .iter()
        .filter(|r| r.outcome.as_deref() == Some("success"))
        .count();
    let linked = rows
        .iter()
        .filter(|r| r.usage_request_id.is_some() && r.is_complete)
        .count();
    let rate = linked as f64 / eligible as f64;
    assert_eq!(eligible, N);
    assert!(
        rate >= 0.99,
        "Eligible Usage Link Rate {rate:.4} < 0.99 (linked={linked} eligible={eligible})"
    );

    state.proxy_service.stop().await.expect("stop proxy");
}

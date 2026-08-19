//! Phase 4 集成测试：AutoTier Shadow Observer
//!
//! 验证内容：
//! 1. Shadow 模式下请求通过，上游 model/provider 与客户端一致（Shadow 不变量）
//! 2. 流式 + 非流式都通过
//! 3. DB 失败不影响请求（文件只读导致 observer 写入失败）
//! 4. Off 模式不写 decision

#[path = "support.rs"]
mod support;

use std::os::unix::fs::PermissionsExt;
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

// ============================================================================
// Mock 上游
// ============================================================================

fn build_non_stream_response(message_id: &str, model: &str) -> Value {
    json!({
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": "mock non-stream reply"}],
        "stop_reason": "end_turn",
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
        ("message_start", json!({
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
        })),
        ("content_block_start", json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        })),
        ("content_block_delta", json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "mock stream reply"}
        })),
        ("content_block_stop", json!({
            "type": "content_block_stop",
            "index": 0
        })),
        ("message_delta", json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn", "stop_sequence": null},
            "usage": {"output_tokens": 9}
        })),
        ("message_stop", json!({"type": "message_stop"})),
    ];

    let mut out = String::new();
    for (event, data) in events {
        out.push_str(&format!("event: {event}\ndata: {data}\n\n"));
    }
    out
}

async fn mock_messages(State(counter): State<Arc<Mutex<u32>>>, body: String) -> Response {
    let parsed: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
    let model = parsed
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let is_stream = parsed
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut c = counter.lock().unwrap();
    let n = *c;
    *c += 1;
    drop(c);

    let message_id = format!("msg_shadow_{n:04}");

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
            .body(build_non_stream_response(&message_id, &model).to_string().into())
            .unwrap()
    }
}

async fn start_mock_upstream() -> (u16, Arc<Mutex<u32>>) {
    let counter = Arc::new(Mutex::new(0u32));
    let app = Router::new()
        .route("/v1/messages", post(mock_messages))
        .with_state(counter.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let port = listener.local_addr().expect("mock local addr").port();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[mock upstream] serve error: {e}");
        }
    });
    (port, counter)
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

async fn wait_for_decisions(db: &Database, expected: i64, timeout: Duration) {
    let start = Instant::now();
    loop {
        let n = count_decisions(db);
        if n >= expected {
            return;
        }
        if start.elapsed() > timeout {
            panic!("等待 decision 超时: 期望 {expected}, 实际 {n}");
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
    home.join(".cc-switch").join("cc-switch.db")
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

    let (upstream_port, _counter) = start_mock_upstream().await;
    let provider = make_claude_provider("p-shadow", "Mock Shadow", upstream_port);
    db.save_provider("claude", &provider).expect("save provider");
    db.set_current_provider("claude", "p-shadow").expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build client");

    let resp = client
        .post(format!("http://127.0.0.1:{}/v1/messages", port))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&request_body("claude-sonnet-4-20250514", false))
        .send()
        .await
        .expect("send request");

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("parse response");
    assert_eq!(body["model"], "claude-sonnet-4-20250514");

    wait_for_decisions(&db, 1, Duration::from_secs(3)).await;
    let decisions = list_recent_decisions(&db);
    assert_eq!(decisions.len(), 1);
    let row = &decisions[0];
    assert_eq!(row.mode, "shadow");
    assert_eq!(row.client_requested_model, "claude-sonnet-4-20250514");
    assert_eq!(
        row.baseline_outbound_model.as_deref(),
        Some("claude-sonnet-4-20250514")
    );
    assert_eq!(
        row.actual_outbound_model.as_deref(),
        Some("claude-sonnet-4-20250514")
    );
    assert_eq!(row.initial_selected_provider.as_deref(), Some("p-shadow"));
    assert!(!row.autotier_mutated_request);
    assert!(!row.is_complete);

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

    let (upstream_port, _counter) = start_mock_upstream().await;
    let provider = make_claude_provider("p-shadow-stream", "Mock Shadow Stream", upstream_port);
    db.save_provider("claude", &provider).expect("save provider");
    db.set_current_provider("claude", "p-shadow-stream").expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build client");

    let resp = client
        .post(format!("http://127.0.0.1:{}/v1/messages", port))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&request_body("claude-sonnet-4-20250514", true))
        .send()
        .await
        .expect("send request");

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

    wait_for_decisions(&db, 1, Duration::from_secs(3)).await;
    let decisions = list_recent_decisions(&db);
    assert_eq!(decisions.len(), 1);
    let row = &decisions[0];
    assert_eq!(row.mode, "shadow");
    assert_eq!(row.client_requested_model, "claude-sonnet-4-20250514");
    assert!(!row.autotier_mutated_request);

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

    let (upstream_port, _counter) = start_mock_upstream().await;
    let provider = make_claude_provider("p-off", "Mock Off", upstream_port);
    db.save_provider("claude", &provider).expect("save provider");
    db.set_current_provider("claude", "p-off").expect("set current provider");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build client");

    let resp = client
        .post(format!("http://127.0.0.1:{}/v1/messages", port))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&request_body("claude-sonnet-4-20250514", false))
        .send()
        .await
        .expect("send request");

    assert_eq!(resp.status(), 200);

    // 等待一会儿确认没有异步写入
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

    let (upstream_port, _counter) = start_mock_upstream().await;
    let provider = make_claude_provider("p-db-fail", "Mock DB Fail", upstream_port);
    db.save_provider("claude", &provider).expect("save provider");
    db.set_current_provider("claude", "p-db-fail").expect("set current provider");

    // 把数据库文件改成只读，模拟写入失败
    let db_path = db_file_path();
    let mut perms = std::fs::metadata(&db_path).expect("get db metadata").permissions();
    perms.set_mode(0o444);
    std::fs::set_permissions(&db_path, perms).expect("set db read-only");

    let info = state.proxy_service.start().await.expect("start proxy");
    let port = info.port;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build client");

    let resp = client
        .post(format!("http://127.0.0.1:{}/v1/messages", port))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&request_body("claude-sonnet-4-20250514", false))
        .send()
        .await
        .expect("send request");

    assert_eq!(resp.status(), 200, "DB 失败不应阻塞请求");
    let body: Value = resp.json().await.expect("parse response");
    assert_eq!(body["model"], "claude-sonnet-4-20250514");

    state.proxy_service.stop().await.expect("stop proxy");
}

//! AutoTier Phase 0 Closure 冒烟证据：Claude 请求链实际运行
//!
//! 通过 `ProxyService::start`（应用启动代理的同一入口，内部走
//! `ProxyServer::new` + `ProxyServer::start`）在随机端口启动真实代理，
//! 用本地 mock 上游实现 Anthropic `/v1/messages`（非流式 JSON / 流式 SSE /
//! tools / 500 错误），再用 reqwest 作为客户端跑 5 个场景：
//!
//!   a. 非流式 POST /v1/messages → 200，响应含 content 和 usage
//!   b. 流式（stream:true）→ SSE 事件序列正确，以 message_stop 结束
//!   c. 含 tools 定义的请求 → 上游收到 tools 字段，tool_use 正常透传
//!   d. 上游返回 500 → 如实记录代理行为（状态码/错误体/是否 failover）
//!   e. Failover：双 Provider 队列，第一家 500，验证是否切到第二家
//!
//! 每个场景后轮询 SQLite `proxy_request_logs` 表并 dump，作为 Usage Finalize 证据。
//!
//! 运行：
//!   cargo test --manifest-path src-tauri/Cargo.toml --test proxy_smoke -- \
//!     --nocapture --test-threads=1

#[path = "support.rs"]
mod support;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use cc_switch_lib::Provider;
use serde_json::{json, Value};
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

// ============================================================================
// Mock 上游（Anthropic /v1/messages）
// ============================================================================

struct MockUpstream {
    /// 名称前缀，用于 message id（区分不同 mock 实例）
    name: String,
    /// 收到的请求体（实际出站请求的 wire 证据）
    requests: Mutex<Vec<Value>>,
    counter: AtomicUsize,
    /// 置 true 时所有请求都返回 500
    always_error: bool,
}

impl MockUpstream {
    fn next_message_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        format!("msg_{}_{:04}", self.name, n)
    }

    fn seen_models(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter_map(|b| b.get("model").and_then(Value::as_str).map(str::to_string))
            .collect()
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
                "usage": {"input_tokens": 15, "output_tokens": 1}
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
        let payload = build_non_stream_response(&message_id, &model, has_tools);
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(payload.to_string().into())
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
// proxy_request_logs dump（Usage Finalize 证据）
// ============================================================================

#[derive(Debug)]
#[allow(dead_code)]
struct LogRow {
    request_id: String,
    provider_id: String,
    model: String,
    request_model: Option<String>,
    session_id: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    total_cost_usd: String,
    status_code: i64,
    latency_ms: i64,
    first_token_ms: Option<i64>,
    is_streaming: i64,
    error_message: Option<String>,
}

fn dump_request_logs(db_path: &Path) -> Vec<LogRow> {
    let conn = rusqlite::Connection::open(db_path).expect("open db for log dump");
    let mut stmt = conn
        .prepare(
            "SELECT request_id, provider_id, model, request_model, session_id,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, status_code, latency_ms, first_token_ms,
                    is_streaming, error_message
             FROM proxy_request_logs ORDER BY created_at, rowid",
        )
        .expect("prepare log query");
    stmt.query_map([], |row| {
        Ok(LogRow {
            request_id: row.get(0)?,
            provider_id: row.get(1)?,
            model: row.get(2)?,
            request_model: row.get(3)?,
            session_id: row.get(4)?,
            input_tokens: row.get(5)?,
            output_tokens: row.get(6)?,
            cache_read_tokens: row.get(7)?,
            cache_creation_tokens: row.get(8)?,
            total_cost_usd: row.get(9)?,
            status_code: row.get(10)?,
            latency_ms: row.get(11)?,
            first_token_ms: row.get(12)?,
            is_streaming: row.get(13)?,
            error_message: row.get(14)?,
        })
    })
    .expect("query logs")
    .collect::<Result<Vec<_>, _>>()
    .expect("collect logs")
}

fn print_log_rows(title: &str, rows: &[LogRow]) {
    println!("--- {title} ({} rows) ---", rows.len());
    for r in rows {
        println!(
            "  request_id={} provider={} model={} request_model={:?} session={:?} \
             in={} out={} cache_read={} cache_create={} cost_usd={} status={} \
             latency_ms={} first_token_ms={:?} streaming={} error={:?}",
            r.request_id,
            r.provider_id,
            r.model,
            r.request_model,
            r.session_id,
            r.input_tokens,
            r.output_tokens,
            r.cache_read_tokens,
            r.cache_creation_tokens,
            r.total_cost_usd,
            r.status_code,
            r.latency_ms,
            r.first_token_ms,
            r.is_streaming,
            r.error_message,
        );
    }
}

/// usage 日志是 tokio::spawn 异步落库的，轮询等待行数增长到期望值
async fn wait_for_log_rows(db_path: &Path, expected: usize, timeout: Duration) -> Vec<LogRow> {
    let start = Instant::now();
    loop {
        let rows = dump_request_logs(db_path);
        if rows.len() >= expected {
            return rows;
        }
        if start.elapsed() > timeout {
            println!(
                "!! 等待日志超时: 期望 {expected} 行，实际 {} 行",
                rows.len()
            );
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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

fn request_body(model: &str) -> Value {
    json!({
        "model": model,
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "say hi"}]
    })
}

// ============================================================================
// 主测试：5 个场景串行跑在同一条真实代理链路上
// ============================================================================

// 测试使用 Mutex 进行串行化，跨 await 持锁是预期行为
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proxy_smoke_claude_request_chain() {
    let overall_start = Instant::now();
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home().to_path_buf();
    let db_path = home.join(".cc-switch").join("cc-switch.db");

    let state = create_test_state().expect("create test state");
    let db = state.db.clone();

    // ---- Mock 上游：good（主）、backup（故障转移第二家）、bad（恒 500） ----
    let (good_port, good_mock) = start_mock_upstream("good", false).await;
    let (backup_port, backup_mock) = start_mock_upstream("backup", false).await;
    let (bad_port, bad_mock) = start_mock_upstream("bad", true).await;
    println!("mock upstreams: good={good_port} backup={backup_port} bad={bad_port}");

    // ---- Provider 配置：p1 指向 good mock，设为 claude 当前供应商 ----
    let p1 = make_claude_provider("p1", "Mock Primary", good_port);
    db.save_provider("claude", &p1).expect("save provider p1");
    db.set_current_provider("claude", "p1").expect("set current");

    // ---- 启动真实代理：listen_port=0 让 OS 分配随机端口 ----
    let mut proxy_config = db.get_proxy_config().await.expect("read proxy config");
    proxy_config.listen_port = 0;
    db.update_proxy_config(proxy_config)
        .await
        .expect("set ephemeral listen port");
    let server_info = state.proxy_service.start().await.expect("start proxy");
    let proxy_port = server_info.port;
    assert_ne!(proxy_port, 0, "代理应绑定到 OS 分配的随机端口");
    println!("proxy started on 127.0.0.1:{proxy_port}");

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build reqwest client");
    let proxy_url = format!("http://127.0.0.1:{proxy_port}/v1/messages");

    let send = |body: Value, session: &str| {
        let client = client.clone();
        let url = proxy_url.clone();
        let session = session.to_string();
        async move {
            client
                .post(&url)
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .header("x-claude-code-session-id", session)
                .json(&body)
                .send()
                .await
                .expect("send request to proxy")
        }
    };

    // ========================================================================
    // 场景 a：非流式 → 200 + content + usage
    // ========================================================================
    println!("\n========== 场景 a：非流式 POST /v1/messages ==========");
    let resp = send(request_body("claude-sonnet-4-6"), "smoke-a").await;
    let status_a = resp.status().as_u16();
    let json_a: Value = resp.json().await.expect("parse scenario a response");
    println!("client status: {status_a}");
    println!("response body: {json_a}");
    assert_eq!(status_a, 200, "场景 a 应返回 200");
    assert!(
        json_a.get("content").and_then(Value::as_array).is_some(),
        "场景 a 响应应含 content"
    );
    assert!(
        json_a.pointer("/usage/input_tokens").is_some()
            && json_a.pointer("/usage/output_tokens").is_some(),
        "场景 a 响应应含 usage"
    );
    let msg_a = json_a.get("id").and_then(Value::as_str).unwrap_or("?");
    let outbound_model_a = good_mock.seen_models().last().cloned().unwrap_or_default();
    let logs = wait_for_log_rows(&db_path, 1, Duration::from_secs(3)).await;
    let row_a = logs.last().expect("场景 a 应落一条日志");
    println!(
        "出站证据: model={outbound_model_a} provider={} message_id={msg_a}",
        row_a.provider_id
    );
    print_log_rows("场景 a 后 proxy_request_logs", &logs);
    assert_eq!(row_a.provider_id, "p1");
    assert_eq!(row_a.status_code, 200);
    assert!(row_a.input_tokens > 0 && row_a.output_tokens > 0);

    // ========================================================================
    // 场景 b：流式 → SSE 序列正确、以 message_stop 结束
    // ========================================================================
    println!("\n========== 场景 b：流式（stream:true） ==========");
    let mut body_b = request_body("claude-sonnet-4-6");
    body_b["stream"] = json!(true);
    let resp = send(body_b, "smoke-b").await;
    let status_b = resp.status().as_u16();
    let content_type_b = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text_b = resp.text().await.expect("read scenario b body");
    println!("client status: {status_b}, content-type: {content_type_b}");
    println!("SSE raw:\n{text_b}");
    assert_eq!(status_b, 200, "场景 b 应返回 200");

    let event_seq: Vec<&str> = text_b
        .lines()
        .filter_map(|line| line.strip_prefix("event: "))
        .collect();
    println!("SSE event sequence: {event_seq:?}");
    assert_eq!(
        event_seq,
        vec![
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop"
        ],
        "场景 b SSE 事件序列应符合 Anthropic 协议"
    );
    assert_eq!(event_seq.last(), Some(&"message_stop"), "应以 message_stop 结束");
    let logs = wait_for_log_rows(&db_path, 2, Duration::from_secs(3)).await;
    let row_b = logs.last().expect("场景 b 应落一条日志");
    let msg_b = row_b
        .request_id
        .strip_prefix("session:")
        .unwrap_or("?")
        .to_string();
    println!(
        "出站证据: model={} provider={} message_id={msg_b}",
        row_b.model, row_b.provider_id
    );
    print_log_rows("场景 b 后 proxy_request_logs（最后一行）", &logs[logs.len() - 1..]);
    assert_eq!(row_b.provider_id, "p1");
    assert_eq!(row_b.status_code, 200);
    assert_eq!(row_b.is_streaming, 1, "流式请求日志应标记 is_streaming=1");
    assert!(row_b.input_tokens > 0 && row_b.output_tokens > 0);
    assert!(row_b.first_token_ms.is_some(), "流式日志应记录 first_token_ms");

    // ========================================================================
    // 场景 c：tools → 上游收到 tools 字段，tool_use 透传
    // ========================================================================
    println!("\n========== 场景 c：含 tools 定义的请求 ==========");
    let mut body_c = request_body("claude-sonnet-4-6");
    body_c["tools"] = json!([{
        "name": "get_weather",
        "description": "Get weather for a city",
        "input_schema": {
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        }
    }]);
    let seen_before = good_mock.requests.lock().unwrap().len();
    let resp = send(body_c, "smoke-c").await;
    let status_c = resp.status().as_u16();
    let json_c: Value = resp.json().await.expect("parse scenario c response");
    println!("client status: {status_c}");
    println!("response body: {json_c}");
    assert_eq!(status_c, 200, "场景 c 应返回 200");

    let upstream_req = good_mock
        .requests
        .lock()
        .unwrap()
        .get(seen_before)
        .cloned()
        .expect("mock 应收到场景 c 请求");
    let tools_seen = upstream_req.get("tools").cloned().unwrap_or(Value::Null);
    println!("上游 mock 收到的 tools 字段: {tools_seen}");
    assert!(
        tools_seen.as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "上游应收到 tools 字段（透传未被剥离）"
    );
    let block_type = json_c
        .pointer("/content/0/type")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert_eq!(block_type, "tool_use", "tool_use 响应块应正常透传给客户端");
    assert_eq!(
        json_c.pointer("/content/0/name").and_then(Value::as_str),
        Some("get_weather")
    );
    let msg_c = json_c.get("id").and_then(Value::as_str).unwrap_or("?");
    let logs = wait_for_log_rows(&db_path, 3, Duration::from_secs(3)).await;
    let row_c = logs.last().expect("场景 c 应落一条日志");
    println!(
        "出站证据: model={} provider={} message_id={msg_c}",
        row_c.model, row_c.provider_id
    );
    print_log_rows("场景 c 后 proxy_request_logs（最后一行）", &logs[logs.len() - 1..]);
    assert_eq!(row_c.status_code, 200);

    // ========================================================================
    // 场景 d：上游 500 → 如实记录代理行为
    // ========================================================================
    println!("\n========== 场景 d：上游返回 500（failover 关闭，单 Provider） ==========");
    let resp = send(request_body("claude-force-500"), "smoke-d").await;
    let status_d = resp.status().as_u16();
    let body_d: Value = resp.json().await.expect("parse scenario d response");
    println!("client status: {status_d}");
    println!("error body: {body_d}");
    println!(
        "实际行为: failover 关闭（auto_failover_enabled=false，max_retries=0）→ \
         代理不切换供应商，把上游状态码与错误体原样透传给客户端"
    );
    assert_eq!(status_d, 500, "failover 关闭时上游 500 应原样透传");
    assert_eq!(
        body_d.pointer("/error/type").and_then(Value::as_str),
        Some("internal_error"),
        "错误体应为上游 JSON 原样透传"
    );
    let logs = wait_for_log_rows(&db_path, 4, Duration::from_secs(3)).await;
    let row_d = logs.last().expect("场景 d 应落一条错误日志");
    println!(
        "出站证据: model={} provider={} message_id=（上游 500，无 message id）",
        row_d.model, row_d.provider_id
    );
    print_log_rows("场景 d 后 proxy_request_logs（最后一行）", &logs[logs.len() - 1..]);
    assert_eq!(row_d.provider_id, "p1");
    assert_eq!(row_d.status_code, 500);
    assert!(
        row_d.error_message.as_deref().unwrap_or("").contains("500"),
        "错误日志应记录 500 错误信息"
    );

    // ========================================================================
    // 场景 e：Failover — 第一家 500，验证切到第二家
    // ========================================================================
    println!("\n========== 场景 e：Failover（pfail=恒 500 → pbackup=正常） ==========");
    let mut pfail = make_claude_provider("pfail", "Mock Failing", bad_port);
    pfail.sort_index = Some(1);
    let mut pbackup = make_claude_provider("pbackup", "Mock Backup", backup_port);
    pbackup.sort_index = Some(2);
    db.save_provider("claude", &pfail).expect("save pfail");
    db.save_provider("claude", &pbackup).expect("save pbackup");
    db.add_to_failover_queue("claude", "pfail").expect("queue pfail");
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
    println!("failover 触发条件: auto_failover_enabled=true + 队列 [pfail, pbackup]；\
              上游 500 属于 Retryable 错误（forwarder.rs:2678 状态码分桶），切换到队列下一家");

    let resp = send(request_body("claude-sonnet-4-6"), "smoke-e").await;
    let status_e = resp.status().as_u16();
    let json_e: Value = resp.json().await.expect("parse scenario e response");
    let msg_e = json_e.get("id").and_then(Value::as_str).unwrap_or("?");
    println!("client status: {status_e}, message_id: {msg_e}");
    println!("response body: {json_e}");
    println!(
        "bad mock 收到 {} 次请求，backup mock 收到 {} 次请求",
        bad_mock.requests.lock().unwrap().len(),
        backup_mock.requests.lock().unwrap().len()
    );
    assert_eq!(status_e, 200, "failover 后客户端应拿到 200");
    assert!(
        msg_e.starts_with("msg_backup"),
        "响应应来自 pbackup 对应的 backup mock，实际 message_id={msg_e}"
    );
    assert_eq!(
        bad_mock.requests.lock().unwrap().len(),
        1,
        "pfail（恒 500）应被尝试 1 次"
    );
    assert!(
        !backup_mock.requests.lock().unwrap().is_empty(),
        "pbackup 应实际接到 failover 后的请求"
    );
    let outbound_model_e = backup_mock.seen_models().last().cloned().unwrap_or_default();

    let logs = wait_for_log_rows(&db_path, 5, Duration::from_secs(3)).await;
    let row_e = logs.last().expect("场景 e 应落一条日志");
    println!("出站证据: model={outbound_model_e} provider={} message_id={msg_e}", row_e.provider_id);
    print_log_rows("场景 e 后 proxy_request_logs（最后一行）", &logs[logs.len() - 1..]);
    assert_eq!(
        row_e.provider_id, "pbackup",
        "failover 实际出站 Provider 应为 pbackup"
    );
    assert_eq!(row_e.status_code, 200);

    // ========================================================================
    // 汇总 dump
    // ========================================================================
    let all_logs = dump_request_logs(&db_path);
    print_log_rows("全部 proxy_request_logs（Usage Finalize 证据）", &all_logs);
    assert_eq!(all_logs.len(), 5, "5 个场景应各落一条日志");

    let elapsed = overall_start.elapsed();
    println!("\n冒烟总耗时: {:.2}s", elapsed.as_secs_f64());

    state.proxy_service.stop().await.expect("stop proxy");
}

# AutoTier Phase 0 — 请求链路图（path-map）

> 基座：`farion1231/cc-switch` @ `30409878bdbdf1c7091c559d6afc367a052da39c`（v3.18.0）
> 所有路径相对 `bases/cc-switch/`；行号基于上述锁定 commit。
> 本图聚焦 Claude Code（Anthropic Messages，`POST /v1/messages`）的流式与非流式路径。

## 1. 总览

```text
Claude Code
  │ POST /v1/messages（ANTHROPIC_BASE_URL 指向本地代理，takeover 写入）
  ▼
axum Server ── handlers::handle_messages ── handle_messages_for_app
  │  ① body 完整读入内存并 parse JSON（原始请求体在此可用）
  │  ② RequestContext::new：Provider 选择 + request_model + session_id 提取
  ▼
RequestForwarder::forward_with_retry ── forward_with_retry_inner ── forward
  │  ③ ModelMapper.apply_model_mapping（per-provider 档位映射，改写 body.model）
  │  ④ 协议转换 transform_claude_request_for_api_format（按需）
  │  ⑤ 认证注入 + 发往上游（hyper/reqwest）
  ▼
process_response
  ├─ 流式：handle_streaming → SseUsageCollector（SSE 事件收集）
  └─ 非流式：handle_non_streaming（整包 JSON 解析）
  ▼
spawn_log_usage（tokio::spawn，非阻塞）
  │  ⑥ request_id = "session:{message_id}"（此刻才存在）
  ▼
UsageLogger::log_with_calculation → INSERT INTO proxy_request_logs（SQLite）
```

## 2. 逐环节真实位置

### 2.1 Server 入口与路由

| 环节 | 文件：行号 | 说明 |
|---|---|---|
| 路由注册 | `src-tauri/src/proxy/server.rs:297` | `.route("/v1/messages", post(handlers::handle_messages))`（另有 `:298` `/claude/v1/messages`） |
| `ProxyState` | `src-tauri/src/proxy/server.rs:34` | 共享状态（db、provider_router、status、config 等） |
| 代理启停 | `src-tauri/src/proxy/server.rs:94`（start）、`:225`（stop） | 监听地址/端口来自 `ProxyConfig` |

### 2.2 Handler

| 环节 | 文件：行号 | 说明 |
|---|---|---|
| `handle_messages` | `src-tauri/src/proxy/handlers.rs:117` | Claude 入口，转 `handle_messages_for_app` |
| `handle_messages_for_app` | `src-tauri/src/proxy/handlers.rs:156` | 主流程函数 |
| 请求体读入+解析 | `handlers.rs:169-175` | 完整 body 字节 → `serde_json::Value`。**原始请求全文在此可用** |
| 构建请求上下文 | `handlers.rs:177-178` | `RequestContext::new(&state, &body, &headers, …)` |
| 调用转发器 | `handlers.rs:194-205` | `forwarder.forward_with_retry(...)` |
| 回填出站真值 | `handlers.rs:218-219` | `ctx.outbound_model`、`ctx.provider` 用转发结果回填 |
| Claude 转换分支 | `handlers.rs:228-243` | `adapter.needs_transform()` → `handle_claude_transform` |
| 透传分支 | `handlers.rs:246-253` | `process_response(..., &CLAUDE_PARSER_CONFIG, ...)` |

### 2.3 Request Context（Provider Resolver + 原始信息捕获）

`src-tauri/src/proxy/handler_context.rs:35-73` — `RequestContext` 贯穿请求生命周期：

| 字段 | 行号 | 内容 |
|---|---|---|
| `provider` / `providers` | `:41` / `:43` | 选中 Provider 与完整 failover 链 |
| `request_model` | `:50` | **客户端原始 model**（`body.model`，`:114-118` 提取） |
| `outbound_model` | `:55` | 映射/转换后实际发往上游的 model（成功后回填） |
| `session_id` / `session_client_provided` | `:64` / `:66` | 会话标识及来源标记 |
| `current_provider_id` | `:48` | 请求开始时的设备级当前 Provider |

- Provider 选择：`RequestContext::new` 内调用 `state.provider_router.select_providers(app_type_str)`（`handler_context.rs:134-144`），实现见 `src-tauri/src/proxy/provider_router.rs:37-109`：failover 开启时按 `get_failover_queue` 顺序 + 熔断器过滤；关闭时仅用当前 Provider。
- Session ID 提取：`extract_session_id`（`src-tauri/src/proxy/session.rs:237-262`）。Claude 优先读 header `x-claude-code-session-id` / `claude-code-session-id`（`session.rs:269`），其次 `metadata.user_id` / `metadata.session_id`，兜底生成 UUID。
- **request_id：此阶段不存在**。基座没有请求期内部 ID（见第 4 节）。

### 2.4 Forwarder / Model Mapping / Protocol Transform

| 环节 | 文件：行号 | 说明 |
|---|---|---|
| `forward_with_retry` | `src-tauri/src/proxy/forwarder.rs:347` | thin wrapper：连接计数 + guard 注入 |
| `forward_with_retry_inner` | `forwarder.rs:387` | failover 循环、熔断器放行（`:439-448`）、逐 Provider 尝试 |
| `forward` | `forwarder.rs:1115` | 单次转发：URL 构建 → 模型映射 → 协议转换 → 认证 → 发送 |
| **Model Mapper** | `forwarder.rs:1162-1169` → `src-tauri/src/proxy/model_mapper.rs:119` | `apply_model_mapping(body, provider)` 改写 `body.model`；`ModelMapping`（`model_mapper.rs:10-17`）为 per-provider 六档（haiku/sonnet/opus/fable/subagent/default），从 provider `settings_config.env` 读取 |
| `[1m]` 标记剥离 | `forwarder.rs:1191-1193` → `model_mapper.rs:149/161` | 本地上文标记不发给上游 |
| **Protocol Transform** | `forwarder.rs:1504-1519` → `src-tauri/src/proxy/providers/claude.rs:342` | `transform_claude_request_for_api_format`；`api_format` 解析见 `claude.rs:38`（`get_claude_api_format`，优先级 meta.apiFormat > settings_config > 默认 anthropic）；`claude_api_format_needs_transform` 见 `claude.rs:96` |
| outbound_model 记录 | `forwarder.rs:1407-1411`（映射后）、`:1584-1591`（出站定稿后刷新） | 计价/归因真值 |
| 熔断器 | `src-tauri/src/proxy/circuit_breaker.rs`（经 `provider_router.rs:119` `allow_provider_request`） | HalfOpen 探测名额管理 |

### 2.5 响应处理与 SSE

| 环节 | 文件：行号 | 说明 |
|---|---|---|
| `process_response` | `src-tauri/src/proxy/response_processor.rs:321` | 按 `is_sse_response` 分流 |
| `handle_streaming` | `response_processor.rs:146` | 复制响应头 → 建 `SseUsageCollector`（`:183`）→ `create_logged_passthrough_stream`（`:189`） |
| `SseUsageCollector` | `response_processor.rs:343-355` | 收集 SSE 事件、记录首事件时间、`finish()` 触发回调（`:405`）；`create_usage_collector` 内 `SseUsageCollector::new` 于 `:493` |
| 透传流（超时控制） | `response_processor.rs:678` | 首字节/静默超时（`handler_context.rs:266` `streaming_timeout_config`） |
| `handle_non_streaming` | `response_processor.rs:208` | 整包读取 → `parser_config.response_parser` 解析 usage（`:237`）→ `spawn_log_usage`（`:254`） |
| Claude usage 解析配置 | `src-tauri/src/proxy/handler_config.rs:139-145` | `CLAUDE_PARSER_CONFIG`：`from_claude_stream_events` / `from_claude_response` |
| `TokenUsage` | `src-tauri/src/proxy/usage/parser.rs:42-54` | 字段：`input_tokens`、`output_tokens`、`cache_read_tokens`、`cache_creation_tokens`（**未拆 5m/1h**）、`model`、`message_id` |

### 2.6 Usage Finalize（收口点）

| 环节 | 文件：行号 | 说明 |
|---|---|---|
| `spawn_log_usage` | `response_processor.rs:557` | **`tokio::spawn`（`:586`）非阻塞**，不受 DB 写失败影响请求；`enable_logging` 关闭时直接 return（`:567-570`） |
| `log_usage_internal` | `response_processor.rs:620` | 组装日志；`request_id = usage.dedup_request_id(dedup_scope)`（`:646`） |
| request_id 生成 | `src-tauri/src/proxy/usage/parser.rs:60-70` | `"session:{message_id}"`（claude 无作用域）；无 `message_id` 时退化为随机 UUID |
| 落库 | `src-tauri/src/proxy/usage/logger.rs:446` `log_with_calculation` → `:651` INSERT；幂等 `INSERT OR REPLACE/IGNORE`（`:165-167`） | 表 `proxy_request_logs`（`src-tauri/src/database/schema.rs:197-211`），主键 `request_id`，含 `request_model`/`model`/`pricing_model`/`session_id`/token/cost/latency/status |
| 成本计算 | `src-tauri/src/proxy/usage/calculator.rs`（`CostCalculator`）+ `model_pricing` 表（`schema.rs:236-241`） | 价格按 `pricing_model` 行解析；缺价有回填逻辑 |

**Usage 收口点结论**：统一收口于 `log_usage_internal`（`response_processor.rs:620`）→ `UsageLogger::log_with_calculation`。AutoTier 的 Routing Decision Finalizer 应挂在与 `spawn_log_usage` 相同的触发点（流式：`SseUsageCollector` 完成回调；非流式：`handle_non_streaming` 的解析后），以 `session:{message_id}` 为关联键反查/回填。

## 3. 各阶段信息可用性矩阵

| 信息 | Handler 入口（`handlers.rs:177` 后） | Forwarder 映射前（`forwarder.rs:1162`） | 上游发送前（`forwarder.rs:1584`） | Usage Finalize（`response_processor.rs:620`） |
|---|---|---|---|---|
| 原始 model | ✅ `ctx.request_model` / `body.model` | ✅（映射前 body） | ⚠️ 仅映射后真值（`outbound_model`）；原始值仍在 `ctx.request_model` | ✅ `request_model` 参数 |
| 原始/目标 provider | ✅ `ctx.provider` + failover 链 | ✅ 当前 attempt 的 provider | ✅ | ✅ `provider_id` |
| request_id | ❌ 不存在（需 AutoTier 自生成） | ❌ | ❌ | ✅ `session:{message_id}` |
| session_id | ✅ `ctx.session_id`（header/metadata 提取或 UUID） | ✅（forwarder 持有副本） | ✅ | ✅ |
| 原始请求体 | ✅ 完整 JSON | ✅（映射前 clone） | ⚠️ 已被转换/过滤 | ❌（仅解析出的 usage） |

## 4. Shadow Decision 最小侵入插入点

**推荐插入点：`handle_messages_for_app` 内、`RequestContext::new` 之后、`create_forwarder` 之前（`src-tauri/src/proxy/handlers.rs:177-194` 之间）。**

理由：

1. 此处同时持有：完整原始请求体（`body`，`:174`）、`ctx.request_model`、`ctx.provider` 与 failover 链（`ctx.get_providers()`）、`ctx.session_id`、`app_type`——Shadow 特征提取与 Slot 候选计算所需信息全部就位。
2. 位于任何改写之前：模型映射（`forwarder.rs:1162`）、`[1m]` 剥离、协议转换（`forwarder.rs:1504`）、私有参数过滤均尚未发生，**天然满足 FR-DEC-003（final == original）**——Shadow 只读不写，`body.clone()` 传给决策器即可。
3. 单点覆盖所有 Claude 入口（`handle_messages` 与 Claude Desktop 网关都经此函数）；对 Codex/Gemini 入口可后续按同模式扩展。
4. 失败安全：决策器用 `tokio::spawn` + panic 隔离调用，异常只影响决策记录，不影响转发（满足 NFR-002、FR-MODE-003）。
5. Off 模式 = 跳过该调用，请求路径与基座逐字节一致（Parity 验证见 `baseline-verification.md` 第 4 节）。

辅助挂点（不改动转发逻辑）：

- **Decision-Usage 关联**：Shadow 决策落库用自生成 `autotier:{uuid}` 主键；在 `SseUsageCollector` 完成回调 / `handle_non_streaming` 解析处（`response_processor.rs:237`、`:493` 回调）捕获 `message_id`，回填决策行的 usage 关联键 `session:{message_id}`，或直接复制 `proxy_request_logs` 的 token/cost 字段完成 Finalize。
- **模式与配置的读取**：决策器开关读取 `autotier_routing_config`（新表），不侵入 `ProxyConfig`。

## 5. Provider-specific Slot 解析涉及模块

按 PRD 第 11 节，Slot 配置存于新表 `autotier_provider_slots(provider_id, slot, model_id, …)`。涉及模块：

| 模块 | 文件 | 改动性质（后续 Phase，不在 Phase 0 实施） |
|---|---|---|
| Schema/迁移 | `src-tauri/src/database/schema.rs:24`（`create_tables_on_conn`）、`:415`（`apply_schema_migrations_on_conn`） | 新增 `autotier_*` 建表 + user_version 迁移 |
| DAO | `src-tauri/src/database/dao/` | 新增 Slot/Decision DAO |
| Slot Resolver（新） | 新模块（建议 `src-tauri/src/proxy/autotier/` 或 `src-tauri/src/services/autotier/`） | 输入 provider_id + slot → model_id；failover 链每个 Provider 独立解析（链来自 `ctx.get_providers()`，`handler_context.rs:250`） |
| 候选计算挂点 | `src-tauri/src/proxy/handlers.rs:177-194` | 仅 Shadow 候选；**不改写** `model_mapper`/`forwarder` |
| 能力/价格来源 | `model_pricing`（`schema.rs:236`）、`src-tauri/src/model_capabilities.rs` | 复用现有定价与能力数据 |

注意：基座 `ModelMapping`（`model_mapper.rs:10`）证明 per-provider 档位模式可行，但 AutoTier Slot 不应复用其 env 配置通道（那是转发改写通道，Shadow 阶段触碰它会违反 FR-DEC-003）；Slot Resolver 必须是独立的只读解析器。

## 6. SQLite Migration 机制

- 引擎：`rusqlite`（同步连接 + `Mutex`，`lock_conn!` 宏）。
- 建表：`Database::create_tables_on_conn`（`src-tauri/src/database/schema.rs:24`），全部 `CREATE TABLE IF NOT EXISTS`，应用启动时执行。
- 版本迁移：`apply_schema_migrations_on_conn`（`schema.rs:415`）读取 `PRAGMA user_version`（`schema.rs:2791`），按 0→1→…→16 顺序应用幂等迁移并 `set_user_version`（`schema.rs:2796`）。当前最新版本 **16**。
- AutoTier 挂接方式：在 `create_tables_on_conn` 追加 `autotier_provider_slots` / `autotier_routing_config` / `autotier_routing_decisions` / `autotier_decision_labels` 的 `CREATE TABLE IF NOT EXISTS`（前缀隔离，符合 PRD 11.6），并将 user_version 提升至 17 做一次性幂等迁移；`database/tests.rs` 已有迁移测试模式可复用（如 `:2946` 起的版本跳跃测试）。
- JSON→SQLite 历史数据迁移独立存在于 `src-tauri/src/database/migration.rs`（与 schema 版本迁移不同机制，AutoTier 不涉及）。

## 7. 关闭 AutoTier 后的 Parity（行为一致性）验证锚点

- 代理 takeover 写入/还原：`src-tauri/src/services/proxy.rs:157`（写入 `ANTHROPIC_BASE_URL` 指向本地代理）、`:925`（`disable_takeover_for_app_sync` 还原）、`proxy_live_backup` 表（`schema.rs:264`）保存接管前配置。
- AutoTier Off 的语义：决策管线完全不执行（在 `handlers.rs:177-194` 插入点处一个 `mode == Off` 短路），转发、映射、转换、usage 落库全部为基座原代码路径。
- 可执行的 Parity 验证方法详见 `baseline-verification.md` 第 4 节（字节级抓包对比 + 基座全量测试 + DB 内容对比）。

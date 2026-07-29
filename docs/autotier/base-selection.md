# AutoTier Phase 0 — 基座选择报告

> 依据 `AutoTier-PRD-v1.0.md` 第 18 节 Phase 0 与第 22 节启动 Prompt。
> 评估日期：2026-07-29。评估方式：克隆最新 main、静态代码走读、真实构建/测试（结果见 `baseline-verification.md`）。

## 0. 结论

**选定基座：`farion1231/cc-switch`**

- 仓库：https://github.com/farion1231/cc-switch （分支 `main`）
- **锁定 Commit SHA：`30409878bdbdf1c7091c559d6afc367a052da39c`**（`chore(presets): add PackyCode backup endpoints`，2026-07-29，版本 v3.18.0）
- 落选基座：`BigStrongSun/ccswitchmulti` @ `2bbe8d204b8b57eb638132c02b8f715a70f530d5`（v3.16.5-22，2026-07-27）

核心理由：AutoTier 是 **Claude Code-first** 的 Shadow 观测产品，需要长期跟随上游演进；ccswitchmulti 是一个 **Codex-first** 的重度分叉（领先上游 535 commits、落后 161 commits），其差异化投入全部在 Codex 多模型路由上，Claude 路径停留在 v3.16.5 时代的上游代码，选择它等于同时背上"落后上游"和"维护无关 Codex 功能"两份成本。

## 1. 候选基座锁定信息

| 项 | farion1231/cc-switch | BigStrongSun/ccswitchmulti |
|---|---|---|
| 分支 | `main` | `main` |
| Commit SHA | `30409878bdbdf1c7091c559d6afc367a052da39c` | `2bbe8d204b8b57eb638132c02b8f715a70f530d5` |
| 版本 | v3.18.0 | v3.16.5-22 |
| 末次提交时间 | 2026-07-29 | 2026-07-27 |
| 定位 | Claude Code / Codex / Gemini CLI 多客户端供应商切换 + 本地代理 | Codex 多模型路由（OpenAI 订阅与 DeepSeek/Qwen/本地/第三方 OpenAI-compatible 合并到 Codex） |
| 与上游关系 | 即上游本体 | fork of farion1231/cc-switch，**ahead 535 / behind 161**（GitHub compare API 与本地 `git rev-list --left-right --count` 双重确认） |
| 与上游代码差异 | — | 479 文件，+122,493 / −5,868 行；其中 `src-tauri/src/proxy/` +17,787 / −1,026 行 |

## 2. 七个评估维度对比

### 2.1 Claude Code-first 路径正确性

**cc-switch 明显占优。**

- Claude 是一等客户端：`POST /v1/messages` 路由（`src-tauri/src/proxy/server.rs:297`）→ `handle_messages`（`src-tauri/src/proxy/handlers.rs:117`）→ `handle_messages_for_app`（`handlers.rs:156`），含 Claude Desktop 网关、Claude 专用协议转换（`src-tauri/src/proxy/providers/claude.rs`，2681 行）、Claude SSE 解析配置（`src-tauri/src/proxy/handler_config.rs:139`）。
- `ModelMapping`（`src-tauri/src/proxy/model_mapper.rs:10-17`）已是 per-provider 的 haiku/sonnet/opus/**fable**/**subagent**/default 六档映射，与 AutoTier 的 Cheap/Mid/Strong 槽位概念同构，可直接复用其配置模式。
- ccswitchmulti 的 `ModelMapping`（其 `model_mapper.rs:10-16`）只有 haiku/sonnet/opus/fable/default 五档，**缺少 `subagent_model`**，且不含上游新增的 `[1m]` 标记处理——Claude 路径是 v3.16.5 时代的上游副本。
- ccswitchmulti 的主投入在 Codex：`handlers.rs` 相对上游 +2,984 行、`forwarder.rs` +3,019 行，几乎全是 Codex 多模型路由（`codex_router_log.rs`、`external_openai_api.rs` 等新增文件），与 AutoTier 的首个目标客户端无关。

### 2.2 上游同步成本

**cc-switch 为零，ccswitchmulti 极高。**

- cc-switch 即上游，跟进 = `git pull`。
- ccswitchmulti ahead 535 / behind 161。落后内容包括 cc-switch v3.16.5 → v3.18.0 之间 Claude 路径的全部修复与特性；每次同步都要对 12 万行级的本地改动做三方合并。PRD R-006（基座升级冲突）在该基座上会成为常态成本。

### 2.3 现有测试与三端构建

两者 CI 结构相同（`.github/workflows/ci.yml`：前端 typecheck/format:check/vitest；后端 `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test`，矩阵 ubuntu-22.04 / windows-latest / macos-latest）。实测（详见 `baseline-verification.md`）：

| 检查 | cc-switch | ccswitchmulti |
|---|---|---|
| `pnpm typecheck` | ✅ 通过 | ✅ 通过 |
| `pnpm format:check` | ✅ 通过 | ❌ **失败**（`src/lib/codexMultiRouterWizard.ts` 未格式化，基座自身遗留问题） |
| `pnpm test:unit` | ✅ 83 文件 / 567 用例全过（~20s） | ✅ 82 文件 / 585 用例全过（~18s） |
| Rust 单测规模 | 2,302 个 `#[test]`（src）+ 117 个（tests/，12 文件） | 2,165 个 `#[test]`（src）+ 11 个集成测试文件 |
| `cargo fmt --check` | ✅ 通过 | ✅ 通过 |
| `cargo clippy -D warnings` | ✅ 通过（4m48s） | ❌ **失败**（`codex_history_migration.rs:3032` `manual_repeat_n`，基座遗留） |
| `cargo test` | ✅ **2,375 passed / 0 failed** | ❌ **lib 1 failed / 2,119+98 passed**（失败为 Codex 转换层用例，基座遗留） |

### 2.4 Provider-aware Model Routing

- 两者都有 per-provider `ModelMapping`（从 provider 的 `settings_config.env` 读取档位模型）。cc-switch 版本更新（六档 + `[1m]` 后缀剥离，`model_mapper.rs:149-172`）。
- ccswitchmulti 额外有 `RequestContext::new_with_provider`（其 `handler_context.rs:192`）显式指定 Provider 的路由能力，但服务于 Codex 场景；AutoTier 的 Provider-specific Slot 解析按 PRD 是新模块（`autotier_provider_slots` 表 + Slot Resolver），两基座接入难度相当，cc-switch 的六档映射语义更贴近。

### 2.5 Usage / Request ID 关联

两者同构（fork 继承）：

- `request_id = "session:{message_id}"`，在**响应完成后**由 `TokenUsage::dedup_request_id` 生成（cc-switch：`src-tauri/src/proxy/usage/parser.rs:60`；ccswitchmulti：其 `parser.rs:34`）。
- `proxy_request_logs` 表以 `request_id` 为主键（cc-switch：`src-tauri/src/database/schema.rs:197-211`），含 `request_model` / `model` / `pricing_model` / `session_id` / token / cost / latency / status 字段；写入幂等（`INSERT OR REPLACE/IGNORE`，cc-switch `usage/logger.rs:165-167`）。
- 重要结论（两基座一致）：**请求进入时没有内部 request_id**，request_id 依赖上游响应的 `message_id`。AutoTier 的 Shadow Decision 需要在 Handler 入口自行生成决策 ID，并在 Usage Finalize 时通过 `message_id`（SSE `message_start` 事件）或 `session:{message_id}` 键完成关联收口。这一点直接决定了 Phase 4/5 的设计。

### 2.6 协议转换与 Streaming 稳定性

- cc-switch 转换层更全更新：`transform_claude_request_for_api_format`（`providers/claude.rs:342`）支持 anthropic/openai_chat/openai_responses/gemini_native 等格式；Codex↔Anthropic/Chat 桥（`transform_codex_anthropic.rs` 3,020 行、`transform_codex_chat.rs` 4,347 行）；SSE 透传带首字节/静默超时与 `SseUsageCollector` 完成守卫（`response_processor.rs:146`、`343`、`678`）。
- ccswitchmulti 继承同一架构但版本更旧（`response_processor.rs` 1,197 行 vs 上游 1,254 行），其新增稳定性投入集中在 Codex 流。

### 2.7 最小长期维护面

- cc-switch：维护面 = AutoTier 自身新增模块（`autotier_*` 表、Decision 管线、UI），上游升级免费获得。
- ccswitchmulti：维护面 = AutoTier 新增模块 + 535-commit 的 Codex 路由分叉 + 追赶 161 个上游提交。对 Claude-first 产品是无谓负担。

## 3. 决策

选定 **farion1231/cc-switch @ `30409878bdbdf1c7091c559d6afc367a052da39c`**，满足 PRD 第 22 节全部七项优先考量。ccswitchmulti 的 Codex 多模型路由实现可作为后续 v0.2+ 支持 Codex 客户端时的参考，但不作为基座。

## 4. 对 PRD 既有假设的修正（Phase 0 发现）

1. **请求期无现成 request_id**：`autotier_routing_decisions.request_id` 不能直接使用基座的 `session:{message_id}`（该键在响应完成后才存在）。Shadow Decision 需在 Handler 入口生成自有 ID（如 `autotier:{uuid}`），Finalize 时以 `session:{message_id}` 反查 `proxy_request_logs` 关联 Usage；或在 SSE `message_start` 捕获 `message_id` 后回填。
2. **cache_creation 未拆 5m/1h**：基座 `TokenUsage` 只有合并的 `cache_creation_tokens`（`usage/parser.rs:42-54`），`model_pricing` 也只有单一 `cache_creation_cost_per_million`（`database/schema.rs:236-241`）。PRD 成本模型（第 13 节）要求的 5m/1h 分拆需在 AutoTier 侧扩展解析与定价，不能假设基座已有。
3. **per-provider 档位映射已有先例**：基座 `ModelMapping` 证明了"按 Provider 配置逻辑档位"在配置与转发链路上可行，Slot Resolver 可复用该模式（从 DB 而非 env 读取）。

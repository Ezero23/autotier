# AMEND-001:Phase 0 Closure 修订（对 PRD v1.0 的第一号修订）

> 状态：已批准（用户裁决 2026-07-29)
> 性质：本文件是 `AutoTier-PRD-v1.0.md` 的修订附录，与 PRD 冲突处以本文件为准；下一版 PRD 应吸收本文件内容。
> 触发：Phase 0 评审发现四处阻断（Shadow 不变量不准确、仓库无基座源码、Decision/Usage 关联竞态、运行证据不完整）及两处表述修正。

## 1. 版本表述修正

选定基座的准确表述为：

```text
farion1231/cc-switch
Commit:          30409878bdbdf1c7091c559d6afc367a052da39c
Package version: 3.18.0
Git describe:    v3.18.0-36-g30409878
```

即：选定 Commit 是 `v3.18.0` Tag 之后第 36 个提交，不是 Tag 本身。此前文档中"cc-switch @ 30409878（v3.18.0）"的写法废止。

## 2. Shadow 阻断级不变量修正

PRD 第 7 节 FR-DEC-003 与 README 中使用的：

```text
final_model == original_model
final_provider == original_provider
```

**废止**。在 cc-switch 基座上，即使完全没有 AutoTier,ModelMapping、Provider Router、Failover、协议转换都可能使客户端原始请求模型 ≠ 最终出站模型、初始选中 Provider ≠ 实际执行 Provider。

新的阻断级不变量：

```text
Shadow 模式必须满足：
  autotier_mutated_request == false
  actual_outbound_model    == baseline_outbound_model
  actual_outbound_provider == baseline_outbound_provider
```

其中 `baseline_outbound_*` 定义为：在相同输入、相同 Provider 状态、相同 Failover 状态下，不运行 AutoTier 决策时，cc-switch 原本会产生的出站结果。

## 3. 模型/Provider 字段语义冻结

领域类型必须区分以下四组字段，不得混用：

| 字段组 | 字段 | 含义 |
|---|---|---|
| 客户端请求 | `client_requested_model`、`initial_selected_provider` | 客户端 body 中的 model；Provider Router 首次选中的 Provider |
| 基线出站 | `baseline_outbound_model`、`baseline_outbound_provider` | 无 AutoTier 时基座本会产生的出站结果（见第 2 节定义） |
| 候选 | `candidate_slot`、`candidate_model`、`candidate_provider` | Shadow 决策推荐的槽位与模型/Provider |
| 实际出站 | `actual_outbound_model`、`actual_outbound_provider` | 本次请求真实发往上游的 model 与执行 Provider（含 Failover 后真值） |

另加标志位：

```text
autotier_mutated_request: bool
```

v0.1 任何请求都必须满足 `autotier_mutated_request == false`。UI 中 Original/Candidate/Final 三组展示分别对应"基线出站 / 候选 / 实际出站"。

## 4. Decision ↔ Usage 关联 ID 契约冻结

废止"Finalize 时通过 `session:{message_id}` 反查 `proxy_request_logs`"的方案（存在异步竞态：Decision Store 与 Usage Logger 各自 `tokio::spawn`，完成顺序不定，DB 反查可能永久 incomplete)。

四个 ID 分开定义，不得混用：

| ID | 语义 | 生成时机 | 可空 |
|---|---|---|---|
| `decision_id` | AutoTier 请求级内部主键 | 请求入口（Handler)生成 | 否 |
| `upstream_message_id` | 上游响应返回的真实 message id | 响应解析（SSE `message_start` / 非流式 JSON) | 是 |
| `usage_request_id` | 基座 Usage 表去重键（`session:{message_id}`) | Usage Logger 生成 | 是 |
| `session_id_hash` | Session 分组评测键 | 入口提取后哈希 | 否（兜底 UUID 的哈希） |

关联流程（无竞态）:

```text
请求入口:    生成 decision_id,挂入 RequestContext,贯穿整个请求生命周期
响应解析:    捕获 upstream_message_id,写回 RequestContext
Usage 收口:  基座 Usage Logger 自行生成 usage_request_id（不改动）
Finalize:   从同一个 RequestContext 取 decision_id 直接 UPDATE 对应 Decision 行,
            回填 upstream_message_id / usage_request_id / actual_outbound_* / usage 数据,
            不做数据库反查
```

## 5. Migration 版本规则

PRD 与 path-map 中"AutoTier 首个 migration 使用 user_version 17"改为规则：

```text
AutoTier migration version = 导入基座当前 user_version + 1
```

在当前锁定基座上计算结果为 17（基座 `PRAGMA user_version = 16`)。Phase 2 开始前必须重新读取实际导入基座的 `user_version`;17 不是产品常量。

## 6. 产品定位与 Agent-agnostic 边界

```text
产品目标:      所有支持自定义 API Endpoint/Provider 的 Agent 用户
v0.1 验证客户端: Claude Code
后续 Adapter:   Codex、OpenCode、Cursor、Cline 等
```

Phase 1 领域类型不得硬编码为 Claude-only。核心类型采用：

```text
AgentType / AppType
AgentAdapter
RequestEnvelope
RoutingFeatures
DecisionInput
DecisionResult
```

Claude 特有的请求解析放入 Claude Adapter;Slot、Decision、Cost、Provider、Policy、Reason Code 保持 Agent-agnostic。本项不扩大 v0.1 实现范围，仅约束核心模型可复用。

## 7. 仓库引导记录（一次性机械操作）

```text
origin   = https://github.com/Ezero23/autotier.git
upstream = https://github.com/farion1231/cc-switch.git
AutoTier base commit = 30409878bdbdf1c7091c559d6afc367a052da39c
```

- 导入方式：`git merge --allow-unrelated-histories 30409878`（保留上游完整 Git 历史；公开历史未重写、未强推）。
- Merge commit:`445e93c7`。与基线 SHA 的 diff 仅含：AutoTier 文档(PRD、README、docs/autotier/*)、`.gitignore` 并集、上游 README 平移为 `README.upstream.md`。
- `README.md` 保持 AutoTier 产品说明；`README.upstream.md` 原样保留上游 README；后续 upstream 合并时 README.md 冲突需人工按本策略解决。
- MIT License 与 attribution 原样保留（`LICENSE` 未改动）。
- 冒烟验证工具：`src-tauri/tests/proxy_smoke.rs` 为仓库引导期唯一新增代码文件，属验证工具而非业务实现。

## 8. 对 Phase 1 的约束汇总

Phase 1(领域类型与配置契约）必须：

1. 按第 3 节四组字段 + `autotier_mutated_request` 定义类型；
2. 按第 4 节四个 ID 定义关联契约；
3. 按第 6 节划分 Agent-agnostic Core 与 Claude Adapter;
4. 不写死 migration 版本号（第 5 节）;
5. Shadow 测试断言第 2 节新不变量，而非 `actual == client_requested`。

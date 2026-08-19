# AutoTier

面向**所有支持自定义 API Endpoint/Provider 的 Agent 用户**的、本地优先的 Shadow 模型路由与成本评测系统。

- 产品目标：Claude Code、Codex、OpenCode、Cursor、Cline 等一切可自定义 Provider 的 Agent
- v0.1 验证客户端：Claude Code(核心类型 Agent-agnostic,Claude 解析走 Adapter，见 AMEND-001 第 6 节）

## 基座与仓库策略

AutoTier 构建于 `farion1231/cc-switch` 之上，保留完整上游历史：

```text
origin   = https://github.com/Ezero23/autotier.git
upstream = https://github.com/farion1231/cc-switch.git
base commit       = 30409878bdbdf1c7091c559d6afc367a052da39c
package version   = 3.18.0
git describe      = v3.18.0-36-g30409878
last upstream sync = v3.20.0 (0b5da510, 2026-08-19)
```

- 上游 README 原样保留在 [`README.upstream.md`](./README.upstream.md);MIT License 与 attribution 未改动
- 同步上游：`git fetch upstream && git merge upstream/main`;`README.md` 冲突按 AMEND-001 第 7 节策略解决

## 当前状态

**Phase 0 Closure（收口）** — 文档见 `docs/autotier/`。尚未包含 AutoTier 业务实现。

v0.1 只做 Shadow 观测：生成可解释的候选路由决策与成本区间。阻断级不变量（AMEND-001 第 2 节）:

```text
autotier_mutated_request == false
actual_outbound_model    == baseline_outbound_model
actual_outbound_provider == baseline_outbound_provider
```

（不假设 actual == client_requested：基座自身的 ModelMapping / Failover / 协议转换本来就可能改变出站值。)

## 文档

| 文档 | 内容 |
|---|---|
| [`AutoTier-PRD-v1.0.md`](./AutoTier-PRD-v1.0.md) | 唯一权威产品与技术合同 |
| [`docs/autotier/amend-001-phase0-closure.md`](./docs/autotier/amend-001-phase0-closure.md) | **AMEND-001**:Shadow 不变量修正、四组字段冻结、ID 契约、Migration 规则、Agent-agnostic 边界、仓库引导记录 |
| [`docs/autotier/base-selection.md`](./docs/autotier/base-selection.md) | 两个候选基座的七维度对比与选型结论 |
| [`docs/autotier/path-map.md`](./docs/autotier/path-map.md) | 基座请求链路图：Handler → Forwarder → Usage Finalize，含 Shadow 插入点 |
| [`docs/autotier/baseline-verification.md`](./docs/autotier/baseline-verification.md) | 构建 / Lint / 测试 / 冒烟实测结果与 Off 模式 Parity 验证方法 |

## 开发纪律（摘自 PRD 与 AMEND-001)

- 一次只执行一个 Phase，每个 Phase 最多修改 5 个文件，通过 Exit Gate 才进入下一阶段
- 默认不保存原始 Prompt / System Prompt / Tool Result / API Key
- 未达质量与成本门禁前，不实现 Live Routing
- `decision_id` / `upstream_message_id` / `usage_request_id` / `session_id_hash` 四个 ID 不得混用
- AutoTier migration version = 导入基座当前 user_version + 1（非常量）

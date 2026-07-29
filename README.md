# AutoTier

面向 Claude Code 的、本地优先的 Shadow 模型路由与成本评测系统。

## 当前状态

**Phase 0（基座选择与可行性 Spike）** — 详见 `docs/autotier/`。

- 唯一权威产品与技术合同：[`AutoTier-PRD-v1.0.md`](./AutoTier-PRD-v1.0.md)
- 选定基座：`farion1231/cc-switch` @ `30409878bdbdf1c7091c559d6afc367a052da39c`（v3.18.0）
- v0.1 只做 Shadow 观测：生成可解释的候选路由决策与成本区间，**绝不改写真实请求的 model/provider**

## 文档

| 文档 | 内容 |
|---|---|
| [`docs/autotier/base-selection.md`](./docs/autotier/base-selection.md) | 两个候选基座的七维度对比与选型结论 |
| [`docs/autotier/path-map.md`](./docs/autotier/path-map.md) | 基座请求链路图：Handler → Forwarder → Usage Finalize，含 Shadow 插入点 |
| [`docs/autotier/baseline-verification.md`](./docs/autotier/baseline-verification.md) | 构建 / Lint / 测试实测结果与 Off 模式 Parity 验证方法 |

## 开发纪律（摘自 PRD）

- 一次只执行一个 Phase，每个 Phase 最多修改 5 个文件，通过 Exit Gate 才进入下一阶段
- Shadow 阻断级不变量：`final_model == original_model` 且 `final_provider == original_provider`
- 默认不保存原始 Prompt / System Prompt / Tool Result / API Key
- 未达质量与成本门禁前，不实现 Live Routing

## 目录说明

- `bases/`（gitignore）：两个候选基座仓库的本地 checkout，仅用于 Phase 0 评估，SHA 已锁定在上述文档中

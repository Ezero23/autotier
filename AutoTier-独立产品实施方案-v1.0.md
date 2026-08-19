# AutoTier 独立产品实施规格 v1.0

> 文档类型：可直接交付开发的产品、架构、数据、测试与发布合同  
> 产品：AutoTier  
> 文档版本：1.0  
> 日期：2026-08-19  
> 首个正式验证客户端：Claude Code  
> 产品形态：本地优先的桌面应用 + 本地 HTTP 代理 + 本地 SQLite  
> 当前仓库：`/Users/zhuxiaolin/Projects/autotier`  
> 当前 HEAD：`a21d48b19ecd92b1e25a18dc955d8ce706898e55`  
> 当前状态：Phase 0–3.1 已提交并推送；Phase 4 Shadow Observer 正在工作树中开发，未提交、未完成、未验证  
> 核心边界：AutoTier 是可独立安装、独立运行的模型路由产品；没有 Potluck Web、Potluck Monitor、Quota Snapshot 或任何外部 Policy Hint 时，核心功能必须完整、稳定、可发布。

---

# 一、文档权威顺序与使用方法

本文件是 AutoTier 从当前仓库状态继续实施的权威合同。

发生冲突时，采用以下优先级：

1. 本规格中标记为“冻结”“阻断级”“发布门禁”的条款。
2. 当前仓库已经提交的代码与测试事实。
3. `docs/autotier/amend-001-phase0-closure.md`。
4. `AutoTier-PRD-v1.1.md`。
5. `AutoTier-PRD-v1.0.md`。
6. 旧版 `autotier-architecture.md`、历史讨论和研究材料。

旧文档中的以下说法一律不得恢复：

- Shadow 下 `actual == client_requested`；
- “安装即可节省 61%–85%”；
- “70% 请求天然适合小模型”；
- 将单轮、同集拟合的 94.8% 当成 Agent 流量泛化准确率；
- 默认保存原始 Prompt；
- 先做 Live Routing，再补测量、回放和回退；
- 让 AutoTier 依赖外部额度产品才能做基础决策。

执行本规格时遵循以下纪律：

- 一次只实施一个 Phase；
- 每个 Phase 最多修改 5 个文件；
- 每个 Phase 完成后运行该阶段要求的全部验证；
- 提交修改文件、需求映射、测试证据、回滚方法和 Exit Gate 结论；
- 等待用户明确批准后才进入下一 Phase；
- 当前未提交的 Phase 4 工作属于用户在制资产，不得覆盖、回退或擅自重写；
- 如发现当前实现与冻结契约冲突，先报告差异，再在获批的 Phase 内修正。

---

# 二、当前事实基线

## 1. 仓库与上游

```text
origin   = https://github.com/Ezero23/autotier.git
upstream = https://github.com/farion1231/cc-switch.git
branch   = main
HEAD     = a21d48b19ecd92b1e25a18dc955d8ce706898e55
```

AutoTier 保留上游完整 Git 历史，MIT License 和 attribution 不得移除。

最初选定基座为：

```text
Commit:          30409878bdbdf1c7091c559d6afc367a052da39c
Package version: 3.18.0
Git describe:    v3.18.0-36-g30409878
```

2026-08-19 已同步至上游 v3.20.0，合并提交为 `ee0fb6ca`。上游 v17 迁移与原 AutoTier v17 迁移发生版本碰撞后，已按“上游当前版本 + 1”的冻结规则将 AutoTier 迁移重编号为 v17 → v18；当前 `SCHEMA_VERSION = 18`。

## 2. 当前提交状态

| 阶段 | 提交 | 状态 | 已有证据 |
|---|---|---|---|
| Phase 0 | `1fb3e1d2`、`4d76a525` | 已完成 | 基座选型、链路图、构建、五场景代理冒烟、Usage 证据、AMEND-001 |
| Phase 1 | `82ad49e8` | 已完成 | Agent-agnostic 类型、四组字段、四 ID、Shadow 不变量、36 个新增单测；当时全量测试通过 |
| Phase 2 | `747800f5`，后经 `ee0fb6ca` 重编号 | 已完成 | 四张 `autotier_*` 表、DAO、迁移、Retention、Label、Slot；上游同步后 v17 → v18 真实库迁移冒烟通过 |
| Phase 3 | `67549906` | 已完成 | Claude Feature Extractor、规则决策器、Reason Code、性能测试、`safe_to_execute=false` |
| Phase 3.1 | `a21d48b1` | 已完成 | 修复 4 类数据正确性问题；`cargo test --lib` 2,778/0；Clippy `-D warnings` 通过 |
| Phase 4 | 无提交 | **在制** | 工作树已有入口 Observer 草稿；尚未完成、尚未评审、尚未运行阶段验证，不得宣称通过 |

## 3. 当前未提交工作树

截至本文审计时：

```text
M  src-tauri/src/autotier/mod.rs
M  src-tauri/src/database/mod.rs
M  src-tauri/src/proxy/handlers.rs
?? src-tauri/src/autotier/observer.rs
```

该草稿已经尝试：

- 在 Claude `/v1/messages` 入口读取 AutoTier 配置；
- 生成 `decision_id`；
- 提取特征并生成 Shadow 决策；
- 异步插入未完成 Decision；
- 不主动改写请求体。

但当前草稿不等于 Phase 4 完成。至少仍需用阶段测试证明：

- Off 真正零提取、零写入、零出站差异；
- Shadow 的基线出站和实际出站来自真实 Forwarder 结果，而不是入口处的客户端请求值；
- Failover 后 Provider 真值正确；
- 配置读取失败不会意外启用 Shadow；
- Decision Store 失败不影响请求；
- 流式、非流式、Tool Use、500、Failover 均保持基座行为；
- 默认持久化数据不含原文或完整 Session ID；
- 未提交代码通过格式、编译、Lint、单测、集成测试和抓包 Parity。

## 4. 当前独立产品化缺口

当前仓库虽已具备 AutoTier 核心模块，但安装身份仍沿用上游：

```text
package name     = cc-switch
Tauri product    = CC Switch
bundle identifier= com.ccswitch.desktop
updater endpoint = 上游发布源
默认数据语义     = ~/.cc-switch
```

因此当前构建不能被称为“可与上游应用安全共存的 AutoTier 独立安装包”。正式发布前必须完成独立产品身份、数据目录、更新源、签名、端口/接管冲突和导入策略，详见后文“兼容、迁移与独立安装”。

---

# 三、最终目标

AutoTier 的最终目标不是“每一轮都挑最便宜的模型”，而是：

> 在不显著降低 Agent 任务成功率的前提下，利用真实请求特征、Provider 能力、缓存成本、失败成本和用户策略，为每个请求给出可解释的能力档位建议；先在 Shadow 中证明，再通过 Canary 渐进执行，并能在任何风险信号出现时回到基线行为。

## 1. v0.1 最终交付

v0.1 是完整可用的 Shadow 产品，必须做到：

- 独立安装、独立数据目录、独立更新；
- 自带 Provider 管理、本地代理、模型发现、模型映射和用量基础能力；
- 用户为每个 Provider 配置 Cheap、Mid、Strong 槽位；
- 对 Claude Code 合格请求生成隐私安全的 Shadow Decision；
- Shadow 永不改变基座本来会产生的实际出站结果；
- 每条建议包含档位、候选模型、分数、置信度、稳定 Reason Code 和不可执行原因；
- 实际 Usage 与 Decision 无竞态关联；
- 分别计算 Input、Output、Cache Read、Cache Write、Retry、Fallback 成本；
- 候选成本只展示 Low/Base/High 投影，不冒充真实节省；
- 支持用户标注、导出、清除、Retention 和 Session Holdout；
- 不依赖 Potluck Monitor、额度快照或网络遥测；
- 外部 Policy Hint 缺失、过期、错误时，行为与纯 AutoTier 完全相同。

## 2. v0.2 条件目标

只有 v0.1 数据和工程门禁全部满足，才允许新建 Live 实施规格：

- Explicit-only；
- 10% 高置信 Canary；
- Session 粘性和缓存保护；
- 失败一轮内回到基线或 Strong；
- 真实节省、质量差异和回退率报告；
- 自动停止 Canary 并退回 Shadow。

## 3. 北极星指标

正式 Live 后的北极星指标是：

```text
Quality-adjusted Net Saving
= 基线实际成本
 - Live 实际成本
 - 失败重试成本
 - Fallback 成本
 - Cache Bust 成本
 - 可量化的质量损失惩罚
```

不得只使用“候选模型单价更低”作为成功指标。

---

# 四、明确不做什么

v0.1 不做：

- 不执行自动模型切换；
- 不提供隐藏 Live Flag；
- 不通过 Header、配置文件或调试菜单绕过 Canary 门禁；
- 不默认保存 Raw Prompt、System Prompt、Tool Schema、Tool Result、文件内容或完整 Session ID；
- 不把候选成本写入 Actual Cost；
- 不声称固定节省比例；
- 不使用单轮 Prompt 数据证明 Coding Agent 效果；
- 不自动在线训练；
- 不调用额外 LLM 来给每个请求分类；
- 不将数据库作为完整会话运行状态；
- 不替代 Provider 的财务账单；
- 不依赖 Potluck Web、Potluck Monitor、Quota Autopilot；
- 不在本阶段实现“临期额度任务收割”；
- 不因为收到外部额度建议就自动开放 Live；
- 不首版同时验证 Claude Code、Codex、OpenCode、Cursor、Cline 全部客户端；
- 不云端托管用户凭据；
- 不做企业 RBAC、多租户和集中审批；
- 不静默覆盖现有代理、现有 Claude 配置或另一应用的接管状态。

---

# 五、赛道依据与可验证参考

## 1. 已有成熟依据

### 模型路由

[RouteLLM](https://arxiv.org/abs/2406.18665) 证明“在强、弱模型之间学习路由，以优化质量与成本”是正式研究方向；其公开框架还包含 Router 评测。

这能证明：模型路由是成立的技术赛道。

它不能证明：AutoTier 当前规则对 Claude Code Agent 流量已经有效，也不能直接搬用论文的节省比例。

### 模型级联和失败升级

[FrugalGPT](https://arxiv.org/abs/2305.05176) 将 LLM Cascade 作为成本与质量联合优化方法。

这能支持 AutoTier 后续“便宜模型先尝试、失败或低置信时升级”的方向。

它不能替代 AutoTier 对 Tool Use、长上下文、缓存和多轮任务的真实 Holdout。

### Provider 路由与能力过滤

[OpenRouter Provider Routing 官方文档](https://openrouter.ai/docs/guides/routing/provider-selection) 展示了 Provider 顺序、Fallback、参数支持、数据策略、吞吐和延迟约束等成熟路由维度。

这能证明 Provider-aware 路由、能力过滤和 Fallback 是成熟网关能力。

它不能证明 AutoTier 的 Cheap/Mid/Strong 分类质量。

### 缓存感知

[Claude Code Prompt Caching 官方文档](https://code.claude.com/docs/en/prompt-caching) 明确说明缓存按模型隔离，切换模型会产生一次 uncached turn；自定义 `ANTHROPIC_BASE_URL` 下缓存行为还取决于实际网关。

[Anthropic Prompt Caching API 文档](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) 明确区分 Cache Read、默认 5 分钟 Cache Write 和可选 1 小时 Cache Write，并说明 Prefix/Breakpoint 规则。

因此 AutoTier 必须把 Cache Read/Write 和切模导致的 Cache Bust 放进成本模型，不能只比较模型 Input 单价。

## 2. AutoTier 自身已有工程依据

Phase 0 的 `src-tauri/tests/proxy_smoke.rs` 已验证：

- Claude 非流式请求；
- Claude SSE；
- Tool Use；
- 上游 500 原样透传；
- Failover 从失败 Provider 切到 Backup；
- Usage 最终归因于真实执行 Provider；
- 成功请求的基座 Usage ID 在响应 Message ID 出现后才形成。

这些证据支持本地代理、RequestContext 和 Usage Finalize 作为实现底座。

它们仍不能证明：

- 分类器的 Strong Recall；
- 真实净节省；
- Live 切模不会降低任务成功率；
- 所有第三方 Provider 能力一致；
- 当前未提交 Phase 4 草稿已经正确。

## 3. 对外宣传边界

Shadow 阶段允许：

- “理论可优化成本区间”；
- “候选路由建议”；
- “未实际执行”；
- “基于当前价格和缓存假设”；
- “需要更多真实数据才能判断是否适合 Live”。

Shadow 阶段禁止：

- “已节省”；
- “保证不降质”；
- “自动省 85%”；
- “大多数请求只需要小模型”；
- 省略样本量、缺失数据和置信度。

---

# 六、统一术语

## 1. Agent / App

发起模型请求的客户端，例如 Claude Code、Codex、OpenCode。

AutoTier Core 保持 Agent-agnostic；v0.1 只把 Claude Code 作为首个正式验证 Adapter。

## 2. Provider

实际提供 API Endpoint、认证和模型服务的连接配置。

同一模型 ID 在不同 Provider 上可能有不同能力、协议、价格、稳定性和缓存语义。

## 3. Model Slot

逻辑能力槽位：

```text
Cheap
Mid
Strong
Long Context（可选）
Background（可选）
```

Slot 不是全局模型名。每个 Provider 独立配置 Slot → Model ID 映射。

## 4. Baseline

在相同输入、相同 Provider 状态、相同 Failover 状态下，不运行 AutoTier 路由决策时，现有执行管线本来会产生的出站结果。

Baseline 不是客户端 Body 中的原始 Model 字符串。

## 5. Candidate

AutoTier 建议的 Slot、Model 和 Provider。Shadow 中 Candidate 仅记录，不提交给执行管线。

## 6. Actual Outbound

本次请求真实发送给上游的 Model 和最终执行 Provider，必须包含 Failover 后真值。

## 7. Decision

一次请求对应的一条 AutoTier 决策记录，包含派生特征、候选、原因、版本、实际 Usage 和完成状态。

## 8. Shadow

运行特征提取和候选决策，但不允许 AutoTier 改写真实请求。

## 9. Canary

只对经过 Allowlist、能力、缓存、成本、置信度和抽样门禁的少量请求执行 Candidate。

## 10. Live

在长期证据和自动回退机制成熟后开放的正式自动路由模式。

## 11. Policy Hint

未来可选的外部建议，例如某 Provider 当前应保护、可放宽或不可用。

Policy Hint 不是命令，不含凭据，不具有绕过 AutoTier 安全门禁的权限；缺失时 AutoTier 完整运行。

---

# 七、产品独立性合同

## 1. 独立运行的最低定义

没有安装或启动任何 Potluck Web 产品时，AutoTier 必须仍能：

- 安装、启动、更新和卸载；
- 管理 Provider；
- 安全保存 API Key；
- 测试连接和发现模型；
- 配置 Provider-specific Slots；
- 接管和恢复 Claude Code 配置；
- 运行本地代理；
- 执行 Off 和 Shadow；
- 记录 Decision 和 Usage；
- 展示成本投影；
- 标注、导出和清除数据；
- 完成 Replay、Session Holdout 和 Live Gate 报告。

## 2. 依赖矩阵

| 环境 | AutoTier 正确行为 |
|---|---|
| 只有 AutoTier | 使用本地请求特征、Provider 能力、用户策略和成本模型；功能完整 |
| AutoTier + 外部 Policy Hint | Hint 通过 Schema、签名/来源、TTL 和置信度校验后，仅作为额外 Policy 输入 |
| 外部产品未安装 | 不报错、不等待、不降低功能；UI 不应制造“缺少组件”的警告 |
| 外部产品离线 | 使用最后有效 Hint 仅到 `expires_at`；过期后立即回到本地策略 |
| Hint Schema 不兼容 | 忽略 Hint，记录稳定错误码；真实请求继续 |
| Hint 与用户锁定冲突 | 用户显式锁定优先 |
| Hint 建议 Strong、能力未知 | 保持 Baseline；Hint 不能绕过能力门禁 |
| Hint 建议 Cheap、任务高风险 | AutoTier 本地风险门禁优先，保持 Baseline/Strong |
| AutoTier Off | 不提取、不写 Decision、不消费 Hint；基座行为不变 |

## 3. 禁止循环依赖

禁止以下结构：

```text
AutoTier 启动必须等待 Monitor
Monitor 开关直接修改 AutoTier 路由模式
AutoTier 将 Provider Key 同步给 Monitor
Hint 生产者等待 AutoTier 的本次决策后再回写同一次请求
外部产品崩溃导致本地代理拒绝请求
```

允许的单向结构：

```text
可选 Hint Producer
        │
        ▼
Policy Hint Adapter（校验、TTL、降级）
        │
        ▼
AutoTier Policy Gate
        │
        ▼
现有 Execution Pipeline
        │
        └──► 本地 Decision/Usage 反馈
```

## 4. 凭据所有权

- AutoTier 管理的 Provider Key 只保存在 AutoTier 本地安全存储；
- Policy Hint 不得含 API Key、OAuth Token、Cookie、Authorization Header；
- AutoTier 不因接入 Hint 而把原始 Prompt 或 Provider Key发送给 Hint 来源；
- 外部系统只能引用稳定的本地映射 ID 或抽象 Provider Key，不获得凭据内容；
- 删除外部 Hint 来源不得删除 AutoTier Provider；
- 删除 AutoTier Provider 不应远程删除外部产品的数据。

---

# 八、模式与状态机

## 1. 用户可见模式

### Off

```text
不提取特征
不运行分类器
不写 Routing Decision
不读取外部 Policy Hint
不改写请求
现有 Provider/Proxy/Usage 功能仍可运行
```

### Shadow

```text
提取派生特征
生成 Candidate
记录 Decision
收口 Actual Usage
AutoTier 不改变请求
只展示理论投影
```

### Canary Live（v0.2，未批准实现）

```text
用户显式 Opt-in
仅 Allowlist 规则
仅高置信请求
能力、缓存、成本、Provider 健康全部通过
稳定抽样
失败一轮内回退
达到停止条件自动回 Shadow
```

### Live（未来）

只有 Canary 长期稳定后开放。首次安装绝不能直接进入 Live。

## 2. 模式迁移

```text
OFF  <──────────────>  SHADOW
                         │
                         │ 数据门禁 + 用户 Opt-in
                         ▼
                    CANARY_LIVE
                         │
                         │ 长期门禁
                         ▼
                       LIVE

任意 Live 状态 ──安全事件──> SHADOW
任意状态 ──用户关闭──> OFF
```

禁止迁移：

- 首次安装 → Canary；
- 首次安装 → Live；
- 数据不足 → Canary；
- 新 Classifier 未经过 Holdout → Canary；
- 能力未知 → Live Candidate；
- Hint 要求 → 绕过用户 Opt-in；
- 调试 Forced Slot → 真实出站。

## 3. 请求生命周期状态机

```text
RECEIVED
  ├─ mode=off / bypass ─────────────► BASELINE_ONLY
  └─ mode=shadow
        ▼
FEATURE_EXTRACTED
        ├─ parse failure ───────────► DECISION_UNSAFE
        ▼
CANDIDATE_DECIDED
        ▼
DECISION_CREATE_QUEUED
        ▼
BASELINE_FORWARDING
        ├─ forward error ───────────► FINALIZE_ERROR
        ├─ non-streaming ───────────► FINALIZE_USAGE
        └─ streaming ───────────────► STREAMING
                                          ├─ normal stop ─► FINALIZE_USAGE
                                          └─ interrupted ─► FINALIZE_PARTIAL

FINALIZE_* ─► COMPLETE | INCOMPLETE_WITH_REASON | STORE_FAILED
```

请求成功与 Decision Store 成功是两个独立结果。Decision Store 失败只能降低观测覆盖率，不得使模型请求失败。

## 4. Shadow 阻断级不变量

冻结定义：

```text
autotier_mutated_request == false
actual_outbound_model    == baseline_outbound_model
actual_outbound_provider == baseline_outbound_provider
```

必须理解：

- `client_requested_model` 可以与 `baseline_outbound_model` 不同；
- `initial_selected_provider` 可以与 `actual_outbound_provider` 不同；
- Model Mapping、协议转换和 Failover 是基座本来就存在的行为；
- Shadow 只保证 AutoTier 没有造成额外差异；
- 入口处不得把客户端值提前写成 Baseline/Actual 真值；
- 基线和实际真值应在真实 Forwarder/Failover 结果可用后回填；
- 发布时必须用 AutoTier Off/Shadow 与无 AutoTier 基线抓包对比，而不能仅信任布尔字段。

---

# 九、用户体验规格

## 1. 首次启动

首次启动必须按以下顺序：

1. 说明 AutoTier 当前默认处于 Shadow；
2. 说明 Shadow 会分析建议但不会替换真实模型；
3. 检测是否存在另一个本地代理或接管应用；
4. 如检测到冲突，只展示选择，不静默覆盖；
5. 添加或导入 Provider；
6. 测试连接；
7. 发现模型，失败时允许手动录入；
8. 为 Cheap、Mid、Strong 选择模型；
9. 验证 Streaming、Tool Use、Context、Vision 和价格状态；
10. 展示将修改的 Claude Code 配置和可恢复备份；
11. 用户确认后接管；
12. 发送一条测试请求；
13. 展示第一张 Shadow Decision 卡。

首次使用固定文案：

> AutoTier 当前在 Shadow 模式下运行。它会在本机分析请求并记录候选模型建议，但不会替换你真实使用的模型。积累足够数据并通过质量门禁后，你才能选择是否开启小比例 Canary。

## 2. Provider 页面

每个 Provider 显示：

- 名称；
- Base URL；
- 认证状态；
- API Format；
- 当前连接状态；
- 最近连接测试时间；
- 模型列表来源和更新时间；
- Cheap/Mid/Strong 映射；
- 能力验证状态；
- 定价来源和更新时间；
- 是否允许作为未来 Live Candidate。

API Key：

- UI 只显示掩码；
- 日志不打印；
- 导出不包含；
- 错误文本不回显；
- 删除 Provider 前提示 Slot、接管和决策历史影响。

## 3. 智能路由页面

v0.1 只允许：

- Off；
- Shadow；
- Forced Candidate Slot 调试覆盖。

Forced Candidate Slot：

- 只改变候选展示；
- 不改变 `actual_outbound_*`；
- UI 必须带“仅建议，不执行”标记；
- 不能通过隐藏配置变成 Forced Live。

页面同时显示：

- Feature Version；
- Classifier Version；
- Policy Version；
- Retention；
- Raw Prompt 开关状态；
- 外部 Policy Hint：未接入 / 有效 / 过期 / 不兼容；
- 当前是否满足 Canary 数据门禁。

## 4. 决策日志

列表筛选：

- 时间；
- Session Hash；
- App Type；
- 客户端模型；
- 基线模型；
- Candidate Slot；
- Candidate Model；
- Actual Model；
- 初始和实际 Provider；
- Reason Code；
- Unsafe Reason；
- Confidence；
- Complete/Incomplete；
- 用户 Label；
- 是否有 Cache Protection；
- 是否使用外部 Hint。

详情页必须分四栏，禁止再使用含糊的 Original/Final：

| 区域 | 展示 |
|---|---|
| 客户端请求 | `client_requested_model`、`initial_selected_provider` |
| 无 AutoTier 基线 | `baseline_outbound_model`、`baseline_outbound_provider` |
| AutoTier 候选 | `candidate_slot`、`candidate_model`、`candidate_provider` |
| 实际出站 | `actual_outbound_model`、`actual_outbound_provider` |

详情页还必须展示：

- `autotier_mutated_request`；
- Reason Code 的本地化解释；
- Unsafe Reason；
- 派生特征摘要；
- 实际 Usage；
- Candidate 成本 Low/Base/High；
- 成本假设；
- 完成状态和缺失字段；
- “Shadow 未执行 Candidate”固定标记。

## 5. 用户标注

枚举冻结：

```text
CORRECT
SHOULD_BE_STRONGER
COULD_BE_CHEAPER
UNSURE
```

可选原因：

```text
TOOL_FAILURE_RISK
LONG_CONTEXT
ARCHITECTURE_REASONING
SIMPLE_FORMATTING
BACKGROUND_TASK
WRONG_PROVIDER_CAPABILITY
CACHE_RISK
OTHER
```

标注不要求用户提交原始 Prompt。

## 6. 清除与导出

用户可分别清除：

- Routing Decisions；
- Labels；
- Raw Prompt（如果曾显式开启）；
- 导出记录；
- AutoTier 配置；
- AutoTier 全部本地数据。

默认清除 AutoTier 数据不得删除基座 Usage；如用户选择同时删除 Usage，必须独立二次确认。

默认导出：

```text
manifest.json
decisions.jsonl
labels.jsonl
```

默认不含原文、Key、Header、完整 Session ID。

---

# 十、系统架构

```text
Claude Code
    │
    ▼
Local Proxy Handler
    │
    ├── 生成 decision_id
    ├── 提取 session_id → salted session_id_hash
    ├── 保存 client_requested_* / initial_selected_provider
    │
    ▼
Claude Agent Adapter
    │
    ▼
Privacy-safe Feature Extractor
    │
    ▼
Decision Engine（纯函数）
    │
    ├── Candidate Slot
    ├── Complexity / Confidence
    ├── Reason Codes
    └── Unsafe Reasons
    │
    ▼
Optional Policy Hint Adapter ── 无 Hint 时直接旁路
    │
    ▼
Policy Gate
    │
    ├── Off: 不运行
    ├── Shadow: 只记录
    ├── Canary: 未来受门禁执行
    └── Live: 未来
    │
    ▼
Provider-aware Slot Resolver
    │
    ├── Slot → Model
    ├── Capability Matrix
    ├── Protocol Compatibility
    ├── Cache Guard
    └── Cost Guard
    │
    ▼
Existing Execution Pipeline
    │
    ├── Provider Router
    ├── Model Mapping
    ├── Protocol Transform
    ├── Circuit Breaker
    ├── Retry / Failover
    └── SSE / Non-streaming
    │
    ▼
Usage Collector
    │
    ▼
Ordered Decision Writer / Finalizer
    │
    ▼
SQLite
    │
    ├── Dashboard
    ├── Labels
    ├── Export
    ├── Replay
    └── Session Holdout Eval
```

## 1. Agent Adapter

职责：

- 将特定 Agent 请求解析为通用 `RequestEnvelope`；
- 提取特定协议中的 Messages、Tools、Cache、Thinking、Image/File 信号；
- 不决定 Provider；
- 不访问数据库；
- 不修改请求。

v0.1 实现 Claude Adapter。后续新增 Codex/OpenCode Adapter 时不得复制 Policy Gate。

## 2. Feature Extractor

职责：

- 纯函数；
- 只输出派生特征；
- 解析失败显式标记 `Unparseable`；
- 不进行网络、数据库或 Provider 访问；
- 不保存原始内容；
- 相同输入和版本输出相同。

## 3. Decision Engine

职责：

- 输入 `DecisionInput`；
- 输出 `DecisionResult` 和 `next_state`；
- 不直接改写请求；
- 不解析 Provider Key；
- 不写数据库；
- Clock 通过参数注入；
- 权重、阈值或规则变化必须 bump `classifier_version`；
- v0.1 `safe_to_execute` 始终 false。

## 4. Policy Gate

唯一有权决定 Candidate 是否能成为执行方案。

Gate 优先级：

```text
用户显式 Bypass / Lock
> 模式限制
> 隐私和协议安全
> Provider/Model Capability
> Tool / Context / Modality
> Cache Protection
> 失败历史和 Circuit Breaker
> 本地质量置信度
> 成本门禁
> 可选 Policy Hint
> 抽样比例
```

Hint 只能降低或调整偏好，不能越过更高优先级安全门禁。

## 5. Slot Resolver

输入：

```text
provider_id
candidate_slot
request_capabilities
```

输出：

```text
candidate_model
capability_status
unsafe_reasons[]
```

Failover 链上的每个 Provider 必须使用自己的 Slot 映射，不能把 Provider A 的 Cheap Model ID 套到 Provider B。

## 6. Ordered Decision Writer

Decision Create 与 Finalize 均不能阻塞真实请求，但也不能依赖两个无序 `tokio::spawn` 的偶然执行顺序。

实现必须满足：

- `decision_id` 在入口生成；
- Create 事件先于同一 `decision_id` 的 Finalize 事件；
- Writer 对同一 Decision 保序；
- Finalize 不通过数据库反查 Usage 来寻找 Decision；
- 缺失 Create 时 Finalize 进入明确重试/死信状态，不静默成功；
- 队列满时请求继续，记录观测丢失计数；
- 应用关闭时在有界时间内 Flush；
- 崩溃恢复后未完成行可被标记和审计；
- 所有写入幂等。

允许实现：单消费者有界 Channel + `DecisionEvent::Create/Finalize`。

禁止实现：

- Create 和 Finalize 分别裸 `tokio::spawn`，无顺序保证；
- Finalize 通过 `session:{message_id}` 反查 Decision；
- 等待远程网络后再写 Create；
- 因 DB 锁冲突拒绝模型请求。

---

# 十一、Feature 与决策契约

## 1. 默认可持久化派生特征

```text
app_type
original_model
user_message_weighted_length
message_count_bucket
user_turn_count_bucket
tool_definition_count
tool_result_count
has_error_tool_result
constraint_count
code_structure_score
has_image_or_file
context_token_bucket
cache_read_tokens
cache_write_tokens
has_effort_or_thinking
recent_complexity_window
session_id_hash
feature_version
extraction_status
```

## 2. 当前 Extractor 事实

当前提交版本：

```text
FEATURE_VERSION = claude-extractor-v0.2
```

当前已实现：

- CJK 加权长度；
- Message/User Turn 分桶；
- Tool Definition/Result 计数；
- Tool Error；
- 代码块、路径和结构信号；
- Image/File，包括 Tool Result 嵌套媒体；
- Thinking/Effort；
- `cache_control` 对 Cache Write 的入口估算；
- `tool_use.input` 进入 Context 估算；
- 缺失/畸形 `messages` → `Unparseable`；
- 性能基准测试。

入口阶段无法知道真实 Cache Read Token，必须在 Usage Finalize 回填；不得把入口的 0 解释为“没有 Cache Read”。

## 3. 当前规则决策器事实

当前提交版本：

```text
CLASSIFIER_VERSION = rules-v0.2
POLICY_VERSION     = shadow-policy-v0.2
```

```text
CAPABILITY_TABLE_VERSION = capability-table-v0.1
COST_MODEL_VERSION = cost-model-v0.1
CACHE_STATS_VERSION = cache-stats-v0.1
```

这三个常量在 Phase 5B 首次交付时引入；版本号随内容变更递增，Decision 与导出记录必须引用当日版本常量（Replay/回放对比的可复现性依赖它们）。

当前规则：

- 加权信号后 clamp 到 `[0,1]`；
- `<0.25` → Cheap；
- `<0.5` → Mid；
- 其余 → Strong；
- 显式 small/haiku/mini/flash/lite 等别名可建议 Cheap；
- `Unparseable` 不推荐 Slot；
- Tool Error、长上下文等进入 Unsafe Reason；
- 能力验证尚未进入决策器，因此当前候选都包含 `CAPABILITY_UNKNOWN`；
- `safe_to_execute` 恒为 false。

这些阈值是 Shadow 起始策略，不是已经验证的最佳策略。

## 4. Reason Code 冻结枚举

```text
SHORT_USER_REQUEST
LOW_CONSTRAINT_COUNT
NO_ACTIVE_TOOL_LOOP
BACKGROUND_METADATA
EXPLICIT_SMALL_MODEL
LONG_CONTEXT
MULTI_FILE_SIGNAL
TOOL_ERROR_PRESENT
HIGH_CONSTRAINT_COUNT
REASONING_SIGNAL
ARCHITECTURE_SIGNAL
MULTIMODAL_INPUT
RECENT_COMPLEXITY_RISING
CACHE_PROTECTION
UNKNOWN_MODEL_CAPABILITY
PROVIDER_SLOT_UNAVAILABLE
USER_FORCED_SLOT
USER_BYPASS
CLASSIFIER_ERROR
```

## 5. Unsafe Reason

至少包含：

```text
CLASSIFIER_ERROR
CONFIG_MISSING
SLOT_INVALID
PROVIDER_NO_CANDIDATE
CAPABILITY_UNKNOWN
COST_MODEL_INCOMPLETE
POLICY_VERSION_INCOMPATIBLE
REQUEST_BODY_UNPARSEABLE
TOOL_USE_NOT_SUPPORTED
PRICE_MISSING
TOOL_ERROR_PRESENT
LONG_CONTEXT_EXCEEDED
HINT_EXPIRED
HINT_SCHEMA_INCOMPATIBLE
CACHE_BUST_RISK
```

新增枚举必须：

- bump 相应 Schema/Policy Version；
- 更新四语言文案；
- 更新导出 Manifest；
- 增加序列化兼容测试；
- 旧客户端遇到未知枚举时安全显示为 Unknown，不崩溃。

## 6. 置信度

Confidence 不是“答案正确概率”，它只表示当前规则信号的一致程度和阈值边际。

UI 必须显示：

> 规则置信度，不代表任务成功率。

只有经过标注数据做 Calibration 后，才允许展示与经验正确率相关的区间。

---

# 十二、能力矩阵与缓存感知

## 1. 每个 Provider Slot 的能力字段

```text
provider_id
slot
model_id
capability_status
supports_tools
supports_streaming
supports_vision
context_limit
api_format
pricing_source
capability_source
verified_at
created_at
updated_at
```

`capability_status` 建议枚举：

```text
unknown
declared
probed
verified
stale
failed
```

只有 `verified` 或满足明确产品政策的 `probed` 才能进入未来 Live。

## 2. 能力门禁

| 请求条件 | Candidate 必须满足 | 不满足时 |
|---|---|---|
| 含 Tools | `supports_tools=true` | 保持 Baseline，`TOOL_USE_NOT_SUPPORTED` |
| Streaming | `supports_streaming=true` | 保持 Baseline |
| 含 Image/File | `supports_vision=true` 或存在已验证降级策略 | 保持 Baseline |
| Context 接近上限 | `context_limit` 留有安全余量 | 保持 Baseline/LongContext |
| API Format 转换 | 转换路径已有集成测试 | 保持 Baseline |
| 能力未知 | 不得 Live | `CAPABILITY_UNKNOWN` |
| 价格未知 | 可 Shadow，不显示金额；Live 取决于 Cost Gate 策略 | `PRICE_MISSING` |

## 3. Cache Guard

候选切换前至少计算：

```text
current_session_model
candidate_model
cache_read_tokens
cache_write_5m_tokens
cache_write_1h_tokens
estimated_rebuild_tokens
cache_ttl_state
session_age
last_model_switch_at
```

v0.1 Shadow：

- 只估算 Cache Bust 风险；
- 不执行切换；
- Candidate Cost 提供“缓存继续命中”和“缓存完全重建”上下界。

未来 Live：

- 默认同一活跃 Session 保持模型粘性；
- 只有质量收益或净成本收益超过 Cache Rebuild 风险才切；
- 模型切换后记录实际 Cache Miss，回填策略评测；
- 一次切换失败不得在下一轮反复抖动；
- 设置最小驻留轮次或冷却窗口。

## 4. Session Affinity

Session Affinity 的来源是内存运行状态，不把完整会话内容写入 AutoTier 数据库。

建议最小状态：

```text
session_id_hash
last_recommended_slot
last_actual_model
recent_complexity_scores
session_request_count
last_switch_at
failure_cooldown_until
```

应用重启后可以从近期 Decision 重建有限状态，但不能恢复原始 Prompt。

---

# 十三、未来可选 Policy Hint 扩展点

## 1. 核心原则

Policy Hint 是未来扩展，不属于 AutoTier v0.1 的核心实现，也不属于当前 Phase 4–9 的发布阻断依赖。

冻结原则：

- 没有 Hint 时，本地策略完整；
- Hint 只读、可选、有 TTL；
- Hint 不含凭据和原始请求；
- Hint 不能直接指定真实出站；
- Hint 不能开启 Canary/Live；
- Hint 不能绕过能力、缓存、质量和用户锁定；
- Hint 过期或不可信时忽略；
- v0.1 最多允许在 Shadow 日志中记录“如果应用 Hint，候选是否会变化”，不得据此执行。

## 2. 建议接口

```rust
pub trait ExternalPolicyHintProvider: Send + Sync {
    fn latest_hint(&self, context: &HintLookupContext)
        -> Result<Option<PolicyHintV1>, HintError>;
}
```

调用必须是本地、非阻塞或有极短超时。请求路径不得同步等待远程网络。

## 3. Policy Hint 线格式：引用权威契约

Policy Hint 的唯一线格式是《Potluck Monitor × AutoTier Quota Autopilot 后续
实施方案 v1.0》第十七章定义的 `quota-policy-hint/v1`（camelCase 字段、四态
state、decisionStatus、constraints、recommendation、六分 confidence）。

本侧实现要求：

- Rust 结构体字段名可保留 snake_case，但必须经 serde rename 与线格式
  camelCase 一一对应；不得出现第二套线格式。
- 原 eligibility/cost_pressure 枚举降级为内部降级判定使用：state=emergency
  映射 unavailable、reserve 映射 protect、harvest 映射 surplus、balanced 映射
  normal；仅用于日志与 UI 展示，不改变线格式。
- 该契约任何字段变更必须先改《后续实施方案》十七章并升版本号，本文只跟随。

## 4. Hint 应用规则

```text
if schema invalid       => ignore
if now >= expires_at    => ignore
if confidence too low   => ignore
if provider mismatch    => ignore
if user locked model    => user lock wins
if local safety blocks  => local safety wins
if hint unavailable     => remove provider from candidate only; baseline fallback remains
if hint protect         => raise downgrade threshold, never force stronger spend
if hint surplus         => may relax cost guard, never relax quality/capability guard
```

## 5. 禁止字段

Policy Hint 禁止包含：

```text
api_key
access_token
refresh_token
cookie
authorization
raw_prompt
system_prompt
tool_result
request_body
response_body
provider_credential_hash
```

## 6. 扩展点验收

- 删除 Hint Adapter 后核心测试仍全绿；
- Adapter Panic/Timeout 不影响请求；
- 过期 Hint 不影响 Candidate；
- Hint 不能把 `safe_to_execute=false` 改成 true；
- Hint 使用情况进入 Decision 的独立字段，而不是混进 Reason 文本；
- 默认构建可完全不包含任何外部产品 SDK。

---

# 十四、四组字段合同（冻结）

以下四组模型/Provider 字段必须在领域类型、数据库、API、导出、UI 和测试中保持同一语义。

## 1. 客户端请求组

```text
client_requested_model
initial_selected_provider
```

含义：

- `client_requested_model`：客户端请求 Body 中的 Model；
- `initial_selected_provider`：Provider Router 首次选中的 Provider；
- 它们只描述入口，不代表真实出站；
- 不因 Failover 或 Model Mapping 被覆盖。

## 2. 基线出站组

```text
baseline_outbound_model
baseline_outbound_provider
```

含义：

- 无 AutoTier 路由改写时，基座本来会发出的结果；
- Shadow 下，应从同一条真实基座执行路径捕获；
- 不允许在 Handler 入口直接复制客户端值来伪装基线；
- Failover 后基线 Provider 是最终实际执行 Provider；
- 请求未达到上游发送点时可以为空。

## 3. 候选组

```text
candidate_slot
candidate_model
candidate_provider
```

含义：

- Decision Engine 先给出逻辑 Slot；
- Slot Resolver 再按 Provider 解析 Model；
- 缺 Slot 或能力未知时 Model/Provider 可以为空；
- Shadow 中 Candidate 永远不能写入真实请求；
- Candidate Cost 永远不能写入 Actual Cost。

## 4. 实际出站组

```text
actual_outbound_model
actual_outbound_provider
```

含义：

- 真正发送给上游的 Model；
- 真正执行请求的 Provider；
- 必须反映 Model Mapping、协议转换和 Failover 后真值；
- 响应回显 Model 优先，Forwarder `outbound_model` 为兜底，客户端 Model 只能是最后兜底且需要注明；
- 无真实出站时可以为空。

## 5. 不变量标志

```text
autotier_mutated_request
```

v0.1 一律为 `false`。

它不是唯一证据。发布仍需字节级请求对比和实际出站字段对比。

## 6. 正确示例

### Model Mapping

```json
{
  "client_requested_model": "claude-sonnet-latest",
  "initial_selected_provider": "provider-a",
  "baseline_outbound_model": "kimi-k2.5",
  "baseline_outbound_provider": "provider-a",
  "candidate_slot": "cheap",
  "candidate_model": "deepseek-v3.2",
  "candidate_provider": "provider-a",
  "actual_outbound_model": "kimi-k2.5",
  "actual_outbound_provider": "provider-a",
  "autotier_mutated_request": false
}
```

### Failover

```json
{
  "client_requested_model": "claude-sonnet-latest",
  "initial_selected_provider": "provider-a",
  "baseline_outbound_model": "claude-sonnet-4.6",
  "baseline_outbound_provider": "provider-backup",
  "candidate_slot": "mid",
  "candidate_model": "claude-sonnet-4.6",
  "candidate_provider": "provider-a",
  "actual_outbound_model": "claude-sonnet-4.6",
  "actual_outbound_provider": "provider-backup",
  "autotier_mutated_request": false
}
```

这里 `initial_selected_provider != actual_outbound_provider` 是合法的基座 Failover，不是 Shadow 违规。

---

# 十五、四个 ID 合同（冻结）

| ID | 语义 | 生成时机 | 可空 | 禁止用法 |
|---|---|---|---|---|
| `decision_id` | AutoTier 请求级主键 | Handler 入口 | 否 | 不得用 Message ID 替代 |
| `upstream_message_id` | 上游真实 Message ID | SSE `message_start` 或非流式 JSON | 是 | 不得作为入口主键 |
| `usage_request_id` | 基座 Usage 去重键 | Usage Logger | 是 | 不得假设总等于 Decision ID |
| `session_id_hash` | Session 分组/评测键 | 入口提取并加盐哈希 | 否 | 不得保存完整 Session ID |

## 1. `decision_id`

- 推荐 UUID v4；
- 必须在 Handler 入口生成；
- 必须挂入 RequestContext 或等价生命周期对象；
- Create、Finalize、日志和导出都以它为 AutoTier 主键；
- 同一请求重试不能产生多个“主 Decision”，Provider Attempt 应作为次数或子事件记录；
- 重复 Create 必须 `ON CONFLICT DO NOTHING`，不得覆盖 Finalize 字段或级联删除 Label。

## 2. `upstream_message_id`

- Claude 非流式从响应 JSON `id` 读取；
- Claude 流式从 `message_start.message.id` 读取；
- 上游错误、连接失败或非法响应时允许为空；
- 不得为“方便关联”合成一个看似上游真实的 Message ID。

## 3. `usage_request_id`

当前基座规则：

```text
Claude/Claude Desktop: session:{message_id}
其他 Agent:            session:{app_type}:{provider_id}:{message_id}
无 message_id:          UUID fallback
```

AutoTier 不修改基座去重语义，只保存其最终结果。

## 4. `session_id_hash`

隐私要求：

- 使用每次安装唯一的随机 Salt/Secret；
- 推荐 HMAC-SHA-256，而不是无盐 SHA-256；
- Secret 只保存在 AutoTier 本地受限目录或系统安全存储；
- 导出 Manifest 写明 Hash 算法与 Scope，但不导出 Secret；
- Secret 轮换后不能跨 Scope 关联；
- 客户端未提供 Session ID 时，对基座生成的 UUID 进行同样哈希；
- 完整 Session ID 不进入 Decision 表、日志或默认导出。

当前未提交 Phase 4 草稿使用直接 SHA-256，只能算在制实现；Phase 4 Exit Gate 前必须按本条完成隐私设计和测试。

## 5. 无竞态关联流程

```text
入口：
  decision_id = UUID
  session_id_hash = HMAC(local_secret, session_id)
  RequestContext.autotier = Some({decision_id, ...})
  queue(CreateDecision)

Forwarder 完成：
  捕获 baseline_outbound_*
  捕获 actual_outbound_*

响应解析：
  捕获 upstream_message_id

Usage Logger：
  生成 usage_request_id
  获得 token / cache / cost / status

Finalize：
  queue(FinalizeDecision {decision_id, ...})
  直接 UPDATE decision_id
  不查询 proxy_request_logs 来反找 Decision
```

## 6. ID 测试

- 同一 Decision 的 Create/Finalize 使用同一 `decision_id`；
- 1000 个并发请求无重复 `decision_id`；
- SSE 与非流式正确提取 Message ID；
- 无 Message ID 时 Usage Fallback 不影响 Decision 主键；
- 同一 Session 在同一 Salt Scope 下 Hash 相同；
- 不同 Salt Scope 下 Hash 不同；
- 日志扫描不出现原 Session ID；
- Finalize 不做数据库反查；
- Create 延迟时 Finalize 不静默丢失。

---

# 十六、数据模型

## 1. `autotier_provider_slots`

```sql
CREATE TABLE autotier_provider_slots (
  provider_id TEXT NOT NULL,
  slot TEXT NOT NULL,
  model_id TEXT NOT NULL,
  capability_status TEXT NOT NULL DEFAULT 'unknown',
  supports_tools INTEGER,
  supports_streaming INTEGER,
  supports_vision INTEGER,
  context_limit INTEGER,
  api_format TEXT,
  pricing_source TEXT,
  capability_source TEXT,
  verified_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (provider_id, slot)
);
```

规则：

- `provider_id + slot` 唯一；
- Cheap/Mid/Strong 齐全才算 Required Slots 完成；
- Provider 删除时明确删除 Slot，不自动删除历史 Decision；
- Slot Model 删除或刷新不存在时标记失效，不静默换成另一个模型；
- 同一 Model 可用于多个 Slot，但 UI 提示档位没有实际区分。

## 2. `autotier_routing_config`

```sql
CREATE TABLE autotier_routing_config (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  mode TEXT NOT NULL DEFAULT 'shadow',
  retention_days INTEGER NOT NULL DEFAULT 30,
  raw_prompt_opt_in INTEGER NOT NULL DEFAULT 0,
  classifier_version TEXT NOT NULL,
  feature_version TEXT NOT NULL,
  policy_version TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

当前默认：

```text
mode               = shadow
retention_days     = 30
raw_prompt_opt_in  = false
feature_version    = claude-extractor-v0.2
classifier_version = rules-v0.2
policy_version     = shadow-policy-v0.2
```

配置读取失败时：

- 请求继续基线执行；
- Observer 当次视为 Off；
- 不允许 `unwrap_or_default()` 把 DB 错误变成默认启用 Shadow；
- 记录脱敏错误与观测丢失计数。

`raw_prompt_opt_in` 在 v0.1 保留为 Schema 兼容字段，但正式 UI 不开放写入，除非另行通过 Raw Data 安全评审。它保持 false。

## 3. `autotier_routing_decisions`

```sql
CREATE TABLE autotier_routing_decisions (
  decision_id TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL,
  completed_at INTEGER,
  app_type TEXT NOT NULL,
  session_id_hash TEXT NOT NULL,
  mode TEXT NOT NULL,

  client_requested_model TEXT NOT NULL,
  initial_selected_provider TEXT,

  baseline_outbound_model TEXT,
  baseline_outbound_provider TEXT,

  recommended_slot TEXT,
  candidate_model TEXT,
  candidate_provider TEXT,

  actual_outbound_model TEXT,
  actual_outbound_provider TEXT,
  autotier_mutated_request INTEGER NOT NULL DEFAULT 0,

  upstream_message_id TEXT,
  usage_request_id TEXT,

  complexity_score REAL,
  confidence REAL,
  reason_codes_json TEXT NOT NULL DEFAULT '[]',
  unsafe_reasons_json TEXT NOT NULL DEFAULT '[]',
  safe_to_execute INTEGER NOT NULL DEFAULT 0,

  feature_json TEXT NOT NULL,
  feature_version TEXT NOT NULL,
  classifier_version TEXT NOT NULL,
  policy_version TEXT NOT NULL,

  actual_input_tokens INTEGER,
  actual_output_tokens INTEGER,
  actual_cache_read_tokens INTEGER,
  actual_cache_write_5m_tokens INTEGER,
  actual_cache_write_1h_tokens INTEGER,
  actual_cost_usd TEXT,

  candidate_cost_low_usd TEXT,
  candidate_cost_base_usd TEXT,
  candidate_cost_high_usd TEXT,
  cost_assumptions_json TEXT NOT NULL DEFAULT '[]',

  status_code INTEGER,
  outcome TEXT,
  retry_count INTEGER NOT NULL DEFAULT 0,
  fallback_count INTEGER NOT NULL DEFAULT 0,
  is_complete INTEGER NOT NULL DEFAULT 0,
  error_code TEXT
);
```

### Create 语义

- 使用 `INSERT ... ON CONFLICT(decision_id) DO NOTHING`；
- 禁止 `INSERT OR REPLACE`；
- 重复 Create 不得覆盖已 Finalize 字段；
- 重复 Create 不得触发 Label 的 Cascade Delete；
- `feature_json` 必须通过 Raw Prompt 字段黑名单测试。

### Finalize 语义

- 只按 `decision_id` UPDATE；
- `None` 字段不覆盖已有值；
- Decision 不存在必须返回错误，不能假装成功；
- Finalize 重复执行保持幂等；
- 成功、失败、流式中断均写 `completed_at` 和 `outcome`；
- 有 `usage_request_id` 或 Token 时，`is_complete=true`；
- 无 Usage 的合法终止保持 `is_complete=false`，但 `completed_at != null`。

### 二维完成状态

当前 Schema 的 `is_complete` 更接近“Usage 已附着”，不能单独代表请求生命周期。

UI 和报表使用：

| `completed_at` | `is_complete` | 含义 |
|---|---:|---|
| null | 0 | 尚未收口或 Writer 丢失 |
| 非 null | 0 | 请求已结束，但没有可用 Usage；可能是连接失败、500 或流式中断 |
| 非 null | 1 | 请求已结束且 Usage 已附着 |
| null | 1 | 非法状态，数据完整性告警 |

不得把“上游错误无 Usage”计入 Decision/Usage 关联失败率分母；报告必须同时给出 eligible 和 ineligible 样本数。

## 4. `autotier_decision_labels`

```sql
CREATE TABLE autotier_decision_labels (
  decision_id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  reason TEXT,
  note TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (decision_id)
    REFERENCES autotier_routing_decisions(decision_id)
    ON DELETE CASCADE
);
```

规则：

- Label Upsert 不改变 Decision；
- Decision 删除可级联删除 Label；
- Retention 删除前 UI 要说明 Label 也会删除；
- 导出保留 Decision/Label 可连接性。

## 5. 索引

至少：

```text
created_at
session_id_hash
client_requested_model
recommended_slot
classifier_version
is_complete
label
```

正式 UI 筛选上线后，根据实际 Query Plan 再决定是否增加：

```text
actual_outbound_model
candidate_model
initial_selected_provider
actual_outbound_provider
outcome
```

不得在没有查询证据时堆积索引。

## 6. Retention

允许值：

```text
0  = 仅内存，不持久化
7
30 = 默认
90
```

规则：

- Retention 以 `created_at` 为准；
- 0 天模式不允许先落盘再异步删除；
- 清除立即生效；
- 清除失败显示明确错误；
- 清除不影响 Provider Key 和基座 Usage；
- 后台 Prune 不阻塞代理；
- Prune 失败不改变 Retention 配置。

## 7. 迁移规则

冻结规则：

```text
AutoTier migration version = 合入目标上游后的当前 user_version + 1
```

当前事实：

```text
上游 v3.20.0 user_version = 17
AutoTier migration         = v17 → v18
当前 SCHEMA_VERSION        = 18
```

后续上游同步步骤：

1. 先读取新上游 `SCHEMA_VERSION`；
2. 列出版本号和表名冲突；
3. 如上游占用 AutoTier 当前版本，先重编号 AutoTier 迁移；
4. 使用真实生产库副本做 Migration Smoke；
5. 证明上游表和 AutoTier 表均保留；
6. 验证降级边界和备份；
7. 再合入主分支。

不得把数字 18 当永久产品常量。

---

# 十七、配置与策略合同

## 1. 本地产品设置

建议逻辑结构：

```json
{
  "routing": {
    "mode": "shadow",
    "retention_days": 30,
    "forced_candidate_slot": null,
    "session_bypass": false
  },
  "privacy": {
    "persist_derived_features": true,
    "raw_prompt_opt_in": false,
    "hash_scope": "install"
  },
  "evaluation": {
    "train_ratio": 0.7,
    "holdout_ratio": 0.3,
    "split_key": "session_id_hash"
  },
  "external_policy_hint": {
    "enabled": false,
    "source": null,
    "max_age_ms": 0
  }
}
```

这只是逻辑配置，不授权创建第二份重复配置。数据库单行配置仍是 Source of Truth，前端通过命令层读取。

## 2. 配置验证

- 未知 Mode：视为 Off；
- `retention_days` 不在允许集合：拒绝保存；
- Version 为空：拒绝启用 Shadow；
- Live Mode 出现在 v0.1 配置：降级为 Shadow 并记录事件；
- `raw_prompt_opt_in=true` 出现在未开放版本：强制 false；
- Hint Source 不存在：禁用 Hint，不影响 AutoTier；
- Slot Model 不存在：Slot Invalid；
- Required Slots 不齐：允许 Shadow 推荐 Slot，但 Candidate Model 为空且不可执行。

## 3. 版本策略

任何以下变化必须 bump：

| 变化 | 版本 |
|---|---|
| 特征字段、提取或分桶变化 | `feature_version` |
| 权重、阈值、模型别名、置信度公式变化 | `classifier_version` |
| Gate 优先级、安全条件、Hint 应用规则变化 | `policy_version` |
| DB 字段/约束变化 | `schema_version` |
| 导出字段或 Manifest 变化 | `export_schema_version` |

历史 Decision 永远保留当时版本，不能在展示时套用最新版本解释。

---

# 十八、成本模型

## 1. 实际成本

```text
actual_cost =
    input_tokens          × input_price
  + output_tokens         × output_price
  + cache_read_tokens     × cache_read_price
  + cache_write_5m_tokens × cache_write_5m_price
  + cache_write_1h_tokens × cache_write_1h_price
  + retry_cost
  + fallback_cost
```

金额使用 Decimal/String 存储，禁止用二进制浮点作为账面金额 Source of Truth。

## 2. 当前基座限制

当前基座 `TokenUsage` 只有合并的：

```text
cache_creation_tokens
```

没有天然拆分：

```text
cache_write_5m_tokens
cache_write_1h_tokens
```

因此 Phase 5 必须选择并记录数据来源：

- 如上游 Usage 明确返回 TTL Breakdown，扩展 Parser 并分别回填；
- 如请求只使用一种已知 TTL，可按请求 Cache Policy 归因；
- 如无法可靠判断，写入 combined/unknown 假设，不得把全部 Token 武断归为 5m 或 1h；
- 候选投影显示数据不完整；
- Actual Cost 沿用基座已验证口径，另列 AutoTier Breakdown Coverage。

## 3. Candidate 投影

### Low

- 输出 Token 不增加；
- 缓存继续命中；
- 无重试；
- Provider 不 Failover。

### Base

- 使用相同任务类别历史 Output Ratio 中位数；
- 使用同 Provider/Model 的 Cache Hit 中位数；
- 使用观察到的 Retry/Fallback 率；
- 价格使用 Decision 时快照。

### High

- 模型切换导致 Cache Rebuild；
- Output Token 上升；
- 发生一次失败重试；
- 必要时回退 Baseline/Strong；
- 包含额外延迟成本说明。

## 4. 价格快照

每个投影至少保存：

```json
{
  "baseline": {
    "provider_id": "p1",
    "model_id": "m1",
    "price_source": "models.dev|builtin|manual|unknown",
    "price_observed_at": 1787126400000,
    "input_per_million": "3.00",
    "output_per_million": "15.00",
    "cache_read_per_million": "0.30",
    "cache_write_5m_per_million": "3.75",
    "cache_write_1h_per_million": "6.00"
  },
  "candidate": {},
  "assumptions": ["CACHE_HIT_PRESERVED"]
}
```

可先存入 `cost_assumptions_json`，但 Schema、最大长度和解析失败行为必须测试。

## 5. Baseline 选择

默认：实际 Baseline Outbound。

可选报告场景：

```text
ACTUAL_BASELINE
USER_SELECTED_STRONG
ALL_STRONG_SCENARIO
ALL_CHEAP_SCENARIO
CUSTOM_BASELINE
```

产品默认不得把“所有请求都用最贵模型”作为唯一 Baseline。

## 6. 真实净节省

Shadow 不存在真实节省。

Live 后：

```text
realized_net_saving =
baseline_replay_cost
- actual_live_cost
- retry_and_fallback_delta
- cache_bust_delta
```

必须同时报告任务质量、样本量和置信区间。

---

# 十九、质量评测与 Measurement-first

## 1. 为什么先测量

当前 115 条旧评测数据：

- 是单轮 Prompt；
- 不含 Tools/System/History；
- 调参与评测同集；
- 不能代表 Claude Code Agent；
- 不能用于发布 Live。

所以 v0.1 的主要产品价值是建立真实、隐私安全、可复现的数据闭环。

## 2. 数据集切分

```text
70% Train/Tune
30% Holdout
```

冻结要求：

- 按 `session_id_hash` 分组；
- 同一 Session 不得跨集合；
- Split Seed 写入 Manifest；
- Holdout 在规则定稿前不可用于调参；
- 报告 Session 数、请求数、类别分布和时间范围；
- 同一用户连续相似请求需要按 Session 隔离，防止泄漏。

## 3. 评测对照组

至少：

```text
ORIGINAL_ROUTING
ALL_STRONG
ALL_CHEAP
RANDOM
DEFAULT_POLICY
TUNED_POLICY
EXPLICIT_ONLY
```

禁止只展示表现最好的策略。

## 4. 必须报告指标

```text
Strong Recall
Unsafe Downgrade Rate
Cheap Precision
Mid Precision/Recall
Overall Accuracy
Strong Ratio
Unknown/Bypass Ratio
Decision Coverage
Decision/Usage Link Rate
Projected Net Saving
Cache-adjusted Saving
Retry/Fallback Delta
Per-rule Distribution
Confidence Calibration
Tool Failure Rate
Session Success Proxy
```

## 5. 质量标签

HTTP 200 不等于任务成功。

可用质量信号按可信度分层：

### 强标签

- 用户明确 Label；
- 测试套件通过/失败；
- Agent 最终任务成功；
- 人工评审；
- 同一任务受控 A/B。

### 中标签

- 一轮内是否需要升级；
- Tool Error；
- 重试；
- 用户立即纠正；
- 会话是否异常终止。

### 弱标签

- HTTP 状态；
- Output Token；
- 延迟；
- 模型自报置信度。

弱标签不能单独决定 Live Gate。

## 6. Canary 数据门禁

以下全部满足才允许规划 Canary：

```text
目标请求数               >= 500
独立 Session             >= 50
高质量 Label/人工评审    >= 200
Holdout Strong Recall     >= 98%
Unsafe Downgrade Rate     <= 2%
Decision Coverage         >= 99.9%
Usage Eligible Link Rate  >= 99%
Cache-adjusted Base Saving>= 15%
Feature+Decision p95      < 1ms
Feature+Decision p99      < 5ms
P0/P1 数据安全缺陷        = 0
用户显式 Opt-in           = true
一键回 Shadow             = 已验证
```

门禁阈值是最低线，不是自动批准。样本分布严重偏斜时仍应拒绝 Canary。

## 7. Canary 停止条件

任一触发立即退回 Shadow：

- Unsafe Downgrade >2%；
- Strong Recall 跌破门槛；
- Tool Failure 显著升高；
- Retry/Fallback 抵消节省；
- Cache Bust 成本超出投影；
- Usage 关联失败 >1%；
- Provider Slot 失效；
- 新 Policy 未经过 Holdout；
- 外部 Hint 异常影响路由；
- 用户报告可复现质量下降；
- 发生任何凭据或 Prompt 泄漏。

---

# 二十、隐私与安全

## 1. 默认不得持久化

```text
Raw User Message
System Prompt
Tool Schema 全文
Tool Result 全文
文件内容
图片 Base64
API Key
Authorization Header
Cookie
完整 Session ID
Request Body
Response Body
```

## 2. 日志允许字段

```text
decision_id
error_code
reason_code
unsafe_reason
model_id
provider_id
token/cost
latency
feature/classifier/policy/schema version
```

Provider ID/Model ID 仍可能是用户自定义敏感文本；支持高级隐私模式对导出中的这些字段做本地映射。

## 3. Raw Prompt

v0.1：

- 后端不提供写入路径；
- 前端不提供开启入口；
- `raw_prompt_opt_in` 固定 false；
- 导出不支持原文。

未来如需开放，必须另做：

- Threat Model；
- 独立加密存储；
- 二次确认；
- 独立 Retention；
- 数据大小上限；
- 清除证明；
- 导出二次确认；
- 日志和崩溃报告审计。

## 4. Session Secret

- 首次启动生成 32 字节随机 Secret；
- macOS 优先 Keychain，Windows Credential Manager，Linux Secret Service；
- 无系统安全存储时使用权限受限文件并明确警告；
- Secret 不进入备份、日志或默认导出；
- Secret 丢失时轮换 Scope，不尝试恢复旧 Session 映射。

## 5. 凭据

- 复用经过验证的本地凭据存储；
- Error 统一脱敏；
- 不将 Key 写入 SQLite Decision；
- 不将 Key 传入 Feature Extractor；
- Policy Hint Adapter 无凭据读取接口；
- 崩溃报告不含请求 Body/Header；
- Clipboard 复制 Key 后提供自动清空提醒或不默认复制。

## 6. 导出 Manifest

```json
{
  "export_schema_version": 1,
  "generated_at": "...",
  "time_range": {},
  "feature_versions": [],
  "classifier_versions": [],
  "policy_versions": [],
  "hash_algorithm": "HMAC-SHA-256",
  "hash_scope": "install",
  "contains_raw_prompt": false,
  "contains_credentials": false,
  "price_sources": [],
  "split_seed": null
}
```

## 7. 安全测试

- 数据库字符串扫描；
- 普通日志、Crash Log 扫描；
- JSONL 导出扫描；
- API Key canary secret 测试；
- Session ID canary secret 测试；
- Prompt canary secret 测试；
- Panic/Error 路径扫描；
- SSE Chunk 不进入日志；
- Policy Hint 恶意超长/嵌套输入限制；
- SQL 参数化；
- 路径遍历和任意导出路径测试。

---

# 二十一、故障安全与回退

## 1. 基本原则

```text
AutoTier 失败 ≠ 模型请求失败
观测失败     ≠ 改走其他模型
Hint 失败     ≠ AutoTier 失败
UI 失败       ≠ Proxy 失败
```

## 2. 故障矩阵

| 故障 | 正确行为 | 记录 |
|---|---|---|
| 配置读取失败 | 当次 Observer Off，走 Baseline | `CONFIG_READ_FAILED` |
| Feature Extractor 解析失败 | 不推荐 Slot，走 Baseline | `REQUEST_BODY_UNPARSEABLE` |
| Decision Engine Panic | 隔离 Panic，走 Baseline | `CLASSIFIER_ERROR` |
| Slot 缺失 | Candidate Model 为空，走 Baseline | `PROVIDER_SLOT_UNAVAILABLE` |
| 能力未知 | 不可执行 | `CAPABILITY_UNKNOWN` |
| DB Create 失败 | 请求继续，覆盖率计数下降 | `DECISION_CREATE_FAILED` |
| DB Finalize 失败 | 有界重试/死信，不影响响应 | `DECISION_FINALIZE_FAILED` |
| DB Locked | Queue/Retry，不阻塞请求 | `DECISION_STORE_BUSY` |
| Writer Queue 满 | 丢观测不丢请求 | `DECISION_QUEUE_FULL` |
| SSE 中断 | 回填部分 Usage/Outcome | `STREAM_INTERRUPTED` |
| Provider 500 | 保留基座 Retry/Failover | 上游状态与实际 Provider |
| Hint 过期 | 忽略 Hint | `HINT_EXPIRED` |
| Hint Schema 错误 | 忽略 Hint | `HINT_SCHEMA_INCOMPATIBLE` |
| UI 崩溃 | Proxy 继续 | Crash Log 脱敏 |

## 3. Live 失败升级（未来）

未来 Canary 中：

- Candidate 发生模型能力类 4xx：一轮内回 Baseline/Strong；
- Candidate Tool Use 失败：回 Baseline/Strong；
- Context Length 风险：不反复用同一 Candidate 重试；
- Provider 5xx/连接失败：交给现有 Failover，但不得把 Provider 故障误判成模型能力不足；
- 升级后保持 Session 粘性，避免下一轮再次降档抖动；
- 每次升级计入真实成本和 Unsafe Downgrade 指标。

## 4. 一键回退

用户切 Off：

- 新请求立即不经过 AutoTier；
- 不要求重启 UI；
- 已在飞请求按开始时模式完成，不中途改写；
- 保留历史 Decision；
- 可选停止本地代理并恢复接管前配置；
- 恢复动作有备份和结果校验。

---

# 二十二、兼容、迁移与独立安装

## 1. 独立产品身份是发布阻断项

正式 AutoTier 安装包必须拥有：

```text
productName       = AutoTier
package name      = autotier
bundle identifier = AutoTier 自有且唯一的反向域名
data root         = ~/.autotier（或各平台标准 App Data 对应目录）
log name          = autotier.log
updater endpoint  = Ezero23/autotier 自有发布源
signing key       = AutoTier 自有发布签名
```

最终 Bundle ID 由发布者确认，例如 `com.ezero.autotier`；确认后写入 ADR，禁止不同平台使用不一致身份。

未完成这些项目时：

- 不得覆盖安装为 AutoTier 正式版；
- 不得使用上游更新源；
- 不得与上游应用争用同一 Bundle ID；
- 不得声称两者可安全共存。

## 2. 数据目录

AutoTier 新安装默认只写 AutoTier 目录。

禁止：

- 静默继续使用 `~/.cc-switch`；
- 在未备份时原地升级上游数据库；
- 把上游配置移动到 AutoTier 后删除原文件；
- 两个进程同时写同一 SQLite；
- AutoTier 卸载时删除上游数据。

## 3. 从现有上游数据导入

首次启动检测到旧目录时，提供：

```text
全新开始
复制 Provider/设置
复制 Provider + Usage 历史
稍后导入
```

导入规则：

1. 只读源目录；
2. 创建源数据库哈希和备份记录；
3. 复制到临时目标；
4. 在临时目标执行迁移；
5. 运行 `PRAGMA integrity_check`；
6. 验证 Provider、Usage、AutoTier 表数量；
7. 原子切换目标；
8. 源目录保持不变；
9. 失败时删除临时目标并显示原因；
10. API Key 是否复制必须符合原安全存储的导出能力，不允许把不可导出的系统凭据降级成明文。

## 4. 代理端口与接管所有权

- AutoTier 使用自己的默认端口或可靠的动态端口策略；
- 启动前检查端口占用和实际进程；
- 检测 Claude Code 当前 `ANTHROPIC_BASE_URL` 是否由其他产品管理；
- 如另一个代理已接管，提供“停止另一个代理”“链式上游（未来）”“取消”选项；
- v0.1 不自动配置代理链；
- 接管前保存精确备份；
- 只恢复 AutoTier 自己写入的值；
- 外部修改后不得用旧备份覆盖；
- 崩溃恢复必须验证 PID、端口和配置所有权。

## 5. 更新源

- AutoTier 只能接收 AutoTier Release Manifest；
- Manifest 和安装包均校验签名；
- 上游同步不等于产品自动更新；
- 上游版本通过开发流程合入，不能让用户客户端直接跳回上游产品；
- 更新前备份数据库；
- 更新失败保留旧可执行文件和数据；
- Release Notes 明确上游基线、AutoTier Schema 和兼容性。

## 6. 上游同步策略

每次同步：

1. `git fetch upstream --tags`；
2. 锁定目标 Tag/SHA；
3. 比较 Proxy、Usage、Schema、Settings、Updater、Branding 差异；
4. 优先解决迁移版本；
5. 保护 AutoTier 四组字段和四 ID；
6. 运行真实库副本迁移；
7. 运行基座全量测试；
8. 运行 AutoTier 专项测试；
9. 运行 Off/Shadow 抓包 Parity；
10. 单独提交 Sync Merge，不与新业务 Phase 混合。

---

# 二十三、分阶段实施计划

## 总纪律

- 下表每一个“子阶段”都是独立审批单元；
- 每个子阶段最多 5 个文件；
- 完成一个子阶段后停止并等待批准；
- 如实际需要第 6 个文件，拆成新子阶段，不得越界；
- 当前 Phase 4 工作树先保护，不得以清理为由删除；
- 大于 300 LOC 的结构重构，先做独立死代码清理 Phase 并单独提交；
- 每次编辑前后复读目标文件；
- 所有阶段都必须报告是否有数据库迁移。

## Phase 0：基座与链路验证

状态：已完成。

证据：

- 基座选型；
- Path Map；
- Baseline Verification；
- AMEND-001；
- 五场景 Proxy Smoke。

不再重复实施。

## Phase 1：领域合同

状态：已完成，提交 `82ad49e8`。

实际文件：4 个，符合上限。

完成：

- Mode/Slot；
- 四组字段；
- 四 ID；
- Routing Decision；
- Reason/Unsafe Reason；
- Session State；
- Shadow 不变量。

## Phase 2：Schema 与 DAO

状态：已完成，提交 `747800f5`，后在 `ee0fb6ca` 完成 v18 重编号。

实际文件：4 个，符合上限。

Phase 3.1 又修复：

- Create 不再 Replace；
- Finalize 检查 affected rows；
- 无 Usage 不标 Complete；
- 默认版本引用引擎常量。

## Phase 3：Extractor 与 Decision Engine

状态：已完成，提交 `67549906`；其后续修正提交 `a21d48b1` 即状态表中单列的 Phase 3.1

每次提交均不超过 4 个文件。

当前只批准 Shadow，不批准执行 Candidate。

## Phase 4A：在制 Observer 代码审计与纯函数收口

状态：**当前正在进行，未完成、未提交、未验证**。

文件预算：最多 5 个。

优先围绕当前 4 个在制文件；如必须增加生命周期字段，只允许增加 1 个 RequestContext 文件。

目标：

- 保存当前 diff；
- 对 `observer.rs` 做完整代码审查；
- `build_shadow_row` 不把入口值冒充 Baseline/Actual；
- Candidate Slot 正确，Candidate Model 未解析时为空；
- 配置错误安全旁路；
- `feature_json` 无原文；
- Phase 4 仍不修改请求；
- 当前在制代码格式、编译和专项单测通过。

Exit Gate：

- 修改文件 ≤5；
- Observer 纯逻辑测试通过；
- `safe_to_execute=false`；
- Baseline/Actual 在入口保持 `None` 或明确 provisional，不写假真值；
- 配置读取失败不启用 Shadow；
- 用户批准后才进入 4B。

## Phase 4B：加盐 Session Hash 与有序 Decision Writer

文件预算：最多 5 个。

目标：

- 安装级 Secret；
- HMAC Session Hash；
- 有界 Decision Event Queue；
- Create/Finalize 同 ID 保序；
- Queue 满/DB 锁不阻塞请求；
- Flush 与丢失计数。

Exit Gate：

- 无盐 Hash 测试被替换；
- 同 Scope 稳定、跨 Scope 不同；
- 1000 并发 Create/Finalize 无静默丢失；
- Panic/Queue Full/DB Locked 请求仍成功；
- 日志无 Session 原文。

## Phase 4C：Claude Handler Shadow 接入

文件预算：最多 5 个。

目标：

- 入口生成 Decision ID；
- RequestContext 贯穿；
- Off 完全旁路；
- Shadow 只读；
- Create 事件入队；
- Forwarder 后捕获真实 Baseline/Actual；
- Error Path 有终态。

Exit Gate：

- 非流式、SSE、Tool Use、500、Failover 五场景通过；
- `autotier_mutated_request=false`；
- Actual = Baseline；
- Initial Provider 与 Failover Actual Provider 正确区分；
- Off 零 Decision；
- Shadow 不增加网络调用。

## Phase 4D：Off/Shadow Parity 专项验证

文件预算：最多 3 个测试/夹具文件。

目标：

- 基线二进制 vs AutoTier Off；
- AutoTier Off vs Shadow；
- 对比上游请求 Method/URL/Header/Body；
- 对比客户端响应和 SSE 事件序列；
- 对比基座 Usage 表；
- 动态字段只使用明确白名单。

Exit Gate：

- 所有场景字节级或语义级一致；
- 无 AutoTier 导致的 Provider/Model 差异；
- 差异报告为空或全部在批准白名单；
- Phase 4 才可标记完成。

## Phase 5A：Usage Finalize 与四 ID 收口

文件预算：最多 5 个。

目标：

- 流式和非流式提取 `upstream_message_id`；
- 捕获 `usage_request_id`；
- 直接按 `decision_id` Finalize；
- 成功、错误、中断都写 Outcome；
- 计算 Usage Eligible 分母；
- 不反查 Decision。

Exit Gate：

- Eligible Link Rate 测试 ≥99%；
- Create/Finalize 竞态测试通过；
- 无 Message ID 路径通过；
- Candidate 不进入 Actual；
- 合法无 Usage 状态展示正确。

## Phase 5B：Actual Cost 与 Candidate Range

文件预算：最多 5 个。

目标：

- Decimal 成本；
- Input/Output/Cache/Retry/Fallback；
- Cache Write TTL 数据完整性；
- Low/Base/High；
- 价格快照；
- 缺价格安全显示。

Exit Gate：

- 金额精度测试；
- 5m/1h/unknown 不混淆；
- Cache Bust High Estimate；
- Candidate Cost 不等于 Actual Saving；
- 历史价格更新不重写历史 Decision。
- CAPABILITY_TABLE_VERSION、COST_MODEL_VERSION、CACHE_STATS_VERSION 三个
  常量已定义并在 Decision/导出中引用。

## Phase 6A：Backend Mode/Slot Commands

文件预算：最多 5 个。

目标：

- 读取/保存 Off/Shadow；
- Slot CRUD；
- Required Slot 校验；
- Capability Status；
- 清除、Retention 基础命令；
- 不暴露 Live Command。

Exit Gate：

- 非法 Mode 拒绝；
- v0.1 Live 值降级；
- Key 不进入响应；
- Command 单测和权限测试通过。

## Phase 6B：Frontend API 与类型

文件预算：最多 5 个。

目标：

- TypeScript 类型；
- Query/Mutation；
- Version 字段；
- 错误枚举；
- 不复制决策逻辑。

Exit Gate：

- Strict Typecheck；
- MSW/Query 测试；
- 未知枚举安全显示；
- 无 Key 泄漏。

## Phase 6C：Provider Slot UI

文件预算：最多 5 个。

目标：

- Cheap/Mid/Strong 选择；
- 能力和价格来源；
- 同 Model 多 Slot 提示；
- 无效 Slot；
- Shadow 解释。

Exit Gate：

- 新用户配置 E2E；
- 键盘操作和焦点顺序；
- Loading/Error/Empty 完整；
- 未验证能力不显示为可 Live。

## Phase 6D：路由设置 UI 与四语言

文件预算：最多 5 个：4 个 Locale + 1 个 Locale Contract Test，或拆出组件子阶段。

目标：

- Off/Shadow；
- Retention；
- Forced Candidate；
- 隐私文案；
- 版本和 Hint 状态；
- 禁止 Live UI。

Exit Gate：

- 四语言 Key 完整；
- 插值变量一致；
- Forced Candidate 明示不执行；
- 可访问性检查通过。

## Phase 7A：Decision List/Detail Backend

文件预算：最多 5 个。

目标：

- 分页；
- 筛选；
- 四组字段；
- 二维完成状态；
- Label Command；
- 清除。

Exit Gate：

- Query Plan 合理；
- 大数据量分页；
- 清除 Cascade 正确；
- Retention 正确。

## Phase 7B：Decision List/Detail UI

文件预算：最多 5 个。

目标：

- 列表、详情；
- 四栏语义；
- Reason/Unsafe Reason；
- Cost Range；
- Label；
- Incomplete 原因。

Exit Gate：

- 不使用含糊 Original/Final；
- Shadow 未执行标记固定；
- 空值和未知枚举安全；
- 大列表性能通过。

## Phase 7C：隐私安全导出

文件预算：最多 5 个。

目标：

- Manifest；
- Decisions JSONL；
- Labels JSONL；
- Schema 校验；
- 路径和大小限制；
- 默认无原文。

Exit Gate：

- Canary Secret 扫描；
- 导出可重新导入 Replay；
- 中断导出不留伪完整文件；
- Manifest 与数据版本一致。

## Phase 8A：Replay Engine

文件预算：最多 5 个。

目标：

- 读取 JSONL；
- 按版本重放；
- 默认/候选策略；
- 确定性；
- 错误行报告。

Exit Gate：

- 相同输入相同输出；
- 不修改生产 DB；
- 不需要 Raw Prompt；
- 版本不兼容明确失败。

## Phase 8B：Session Holdout 与 Metrics

文件预算：最多 5 个。

目标：

- Session Group Split；
- Seed；
- Baselines；
- Strong Recall/Unsafe Downgrade；
- Cache-adjusted Saving；
- 分布和样本量。

Exit Gate：

- Session 零泄漏；
- Holdout 不参与调参；
- 指标可复算；
- 小样本不显示“通过”。

## Phase 9A：独立包身份与更新源

文件预算：最多 5 个。

目标：

- Package Name；
- Product Name；
- Bundle ID；
- 更新源；
- 签名和 Release Metadata。

Exit Gate：

- 不再指向上游更新；
- 安装包显示 AutoTier；
- 与上游 Bundle 可共存；
- 更新签名验证。

## Phase 9B：独立数据目录与导入

文件预算：最多 5 个。

如需要更多路径模块，继续拆为 9B.1/9B.2，每个仍 ≤5。

目标：

- AutoTier 数据目录；
- 旧目录只读检测；
- Copy-only 导入；
- 临时迁移和原子切换；
- Integrity Check；
- 回滚。

Exit Gate：

- 两应用不会并发写同库；
- 失败不破坏源；
- 真实库副本导入通过；
- 卸载不删除源应用数据。

## Phase 9C：品牌文案与本地存储 Key

文件预算：最多 5 个。

机械替换必须分批，禁止一次跨越所有文件。

目标：

- 四语言产品名；
- Theme/Last View Storage Key；
- 日志/备份名称；
- About/Welcome；
- Locale Coverage。

Exit Gate：

- UI 无错误上游品牌；
- 必须保留的 Attribution 在 About/License 中存在；
- 旧 LocalStorage 可选迁移；
- 四语言测试通过。

## Phase 9D：端口、接管和共存 E2E

文件预算：最多 5 个测试/实现文件；超出继续拆分。

目标：

- 端口所有权；
- 另一个代理检测；
- 配置备份与恢复；
- 外部修改保护；
- 崩溃恢复；
- 两应用共存。

Exit Gate：

- 不静默覆盖；
- 只恢复 AutoTier 自己写入的值；
- PID/端口真实对账；
- macOS/Windows/Linux CI 或平台验证完成。

## Phase 10A：v0.1 Fresh Install 验证

文件预算：验证原则上 0；发现问题另开不超过 5 文件的修复 Phase。

流程：

1. 新机器/新用户目录；
2. 安装；
3. Provider；
4. Slots；
5. Claude 接管；
6. 五场景请求；
7. Decision/Usage；
8. Label；
9. Export/Replay；
10. Clear；
11. Off；
12. 恢复配置；
13. 卸载；
14. 更新测试。

## Phase 10B：三端与发布审计

文件预算：验证 0；修复单独分 Phase。

Exit Gate：后文所有发布门禁通过。

## Phase 11：Policy Hint Adapter

状态：未来可选，不属于 v0.1 核心。

前置条件：

- AutoTier v0.1 独立运行已发布；
- Extension Contract 单测冻结；
- 无依赖回归；
- 用户另行批准。

首次只进入 Shadow，不进入 Live。

## Phase 12：Canary/Live

状态：禁止提前实施。

只有数据门禁、工程门禁和用户批准全部满足后，重新写独立 PRD；不能把本节当作代码授权。

## 文件上限自检

| 子阶段 | 最大文件数 |
|---|---:|
| 4A | 5 |
| 4B | 5 |
| 4C | 5 |
| 4D | 3 |
| 5A | 5 |
| 5B | 5 |
| 6A | 5 |
| 6B | 5 |
| 6C | 5 |
| 6D | 5 |
| 7A | 5 |
| 7B | 5 |
| 7C | 5 |
| 8A | 5 |
| 8B | 5 |
| 9A | 5 |
| 9B.x | 5 |
| 9C.x | 5 |
| 9D.x | 5 |
| 10A/10B | 0；修复另开 ≤5 |
| 11 | 另行拆分，每段 ≤5 |
| 12 | 未授权 |

---

# 二十四、测试矩阵

## 1. Feature Extractor

- 空消息；
- 缺失 `messages`；
- `messages` 类型错误；
- 中文、英文、混合文本；
- 多轮；
- Tools；
- `tool_use.input`；
- Tool Result；
- Tool Error；
- 嵌套 Image/Document；
- System Array/String；
- Cache Control；
- Thinking/Effort；
- 超长输入；
- 非标准 Content Block；
- Raw Secret 不进入 Features；
- p95/p99。

## 2. Decision Engine

- 确定性；
- 输入不变；
- Next State；
- Threshold 边界；
- Explicit Small Alias；
- Unparseable；
- Tool Error；
- Long Context；
- Multimodal；
- Cache Protection；
- Capability Unknown；
- `safe_to_execute=false`；
- Reason Code 序列化；
- Version Bump Contract。

## 3. Shadow

- Off 零提取；
- Off 零 Decision；
- Shadow Create；
- Baseline/Actual 真值；
- Model Mapping；
- Failover；
- 非流式；
- SSE；
- Tool Use；
- 500；
- Store Error；
- DB Locked；
- Queue Full；
- Proxy Restart；
- UI Closed；
- `autotier_mutated_request=false`；
- 字节级 Parity。

## 4. 数据库

- Fresh Schema；
- v17 → v18；
- 真实生产库副本；
- Migration 幂等；
- Create DO NOTHING；
- Label 保留；
- Finalize Missing ID；
- Finalize 幂等；
- No Usage Incomplete；
- Usage Complete；
- Cascade；
- Retention 0/7/30/90；
- Clear；
- Concurrent Write；
- Crash Recovery；
- Integrity Check。

## 5. Cost

- Input；
- Output；
- Cache Read；
- 5m Write；
- 1h Write；
- Unknown Write TTL；
- Retry；
- Fallback；
- Price Missing；
- Decimal Precision；
- Snapshot；
- Cache Bust；
- Low/Base/High 单调性；
- Candidate 不写 Actual。

## 6. 隐私

- Prompt Canary；
- API Key Canary；
- Authorization Canary；
- Session ID Canary；
- DB Scan；
- Log Scan；
- Crash Scan；
- Export Scan；
- HMAC Scope；
- Secret Rotation；
- Clear 后不可查询；
- Retention 0 零落盘。

## 7. Replay/Eval

- JSONL Schema；
- Manifest；
- Session Split；
- Seed Determinism；
- No Leakage；
- Version Incompatible；
- Malformed Row；
- Baseline 全部存在；
- Metrics 可复算；
- Holdout 不调参；
- 小样本警告。

## 8. 独立安装

- Unique Bundle ID；
- Unique Data Root；
- Unique Updater；
- 与上游并装；
- 旧数据 Copy Import；
- 源不变；
- 端口冲突；
- 接管冲突；
- 恢复外部修改；
- 卸载数据保留；
- Update/Rollback；
- 三端路径。

## 9. Optional Hint

- 无 Adapter；
- No Hint；
- Valid；
- Expired；
- Future Schema；
- Low Confidence；
- Provider Mismatch；
- Malicious Oversize；
- Timeout/Panic；
- User Lock；
- Local Safety Wins；
- Hint 不能开启 Live。

---

# 二十五、强制验证命令

每个涉及 Rust/前端的阶段按范围运行；发布阶段运行全部：

```bash
pnpm typecheck
pnpm format:check
pnpm test:unit
pnpm run build:renderer

cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo build --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --test proxy_smoke
```

注意：

- `vitest.config.ts` 必须继续排除 `bases/**`，避免评估副本双 React 被误收集；
- 合并上游修改 `package.json` 后先更新依赖；
- 本机代理异常时只按已记录方式处理网络，不修改项目逻辑绕过测试；
- 三端发布以 CI Artifact 和安装冒烟为准；
- 当前未提交 Phase 4 未运行完整验证，本文不把它标为通过。

每阶段报告：

```text
命令
exit code
passed / failed / ignored
耗时
环境
是否存在 Warning
失败是否由本阶段引入
```

---

# 二十六、发布门禁

## 1. 功能

- Fresh Install 完成；
- Provider 添加、测试、发现模型；
- Required Slots；
- Off/Shadow；
- Decision Coverage ≥99.9%；
- Eligible Usage Link ≥99%；
- 四组字段正确；
- 四 ID 正确；
- Label/Retention/Clear；
- Export/Replay/Eval；
- Optional Hint 缺失不影响功能。

## 2. Shadow 安全

- 100% `autotier_mutated_request=false`；
- 100% Actual Outbound = Baseline Outbound；
- 五场景 Parity；
- Off 零 Decision；
- Shadow 不新增网络调用；
- Store/Classifier 失败不影响请求；
- 无隐藏 Live 路径。

## 3. 隐私

- DB 无 Raw Prompt；
- Log 无 Body/Header/Key；
- Export 无 Raw Prompt/Key/Session 原文；
- Session HMAC 加盐；
- Retention 0 零落盘；
- Clear 有证明；
- Hint 无敏感字段。

## 4. 工程

- Compiler/Typecheck；
- Format/Lint；
- 全部单测；
- 全部集成；
- 基座回归；
- Proxy Smoke；
- 三端 CI；
- 安装包签名；
- Updater 签名；
- 无未解决 P0/P1；
- 工作树干净；
- Migration 备份和真实库副本通过。

## 5. 独立产品

- Product Name = AutoTier；
- Bundle ID 唯一；
- 数据目录唯一；
- 更新源唯一；
- 可与上游应用并装；
- 不共写 SQLite；
- 端口/接管冲突不静默覆盖；
- 导入是 Copy-only；
- License/Attribution 完整。

## 6. 文案

- Shadow 只说理论投影；
- 不出现未经验证节省百分比；
- Confidence 不冒充成功概率；
- Candidate/Actual 清楚区分；
- 已知限制完整；
- Release Notes 明确仅 Shadow。

---

# 二十七、风险登记

| 风险 | 严重度 | 触发 | 缓解 |
|---|---:|---|---|
| Shadow 改写请求 | P0 | Actual ≠ Baseline | 不变量、抓包、自动阻断发布 |
| 入口值冒充出站真值 | P0 | Mapping/Failover 场景错 | Forwarder 后捕获，不在入口预填 |
| Create/Finalize 竞态 | P0 | Finalize 找不到行 | 有序 Writer、直接 Decision ID、重试/死信 |
| Session 可被字典反推 | P0 | 无盐 Hash | HMAC + 安装 Secret |
| Prompt/Key 泄漏 | P0 | DB/Log/Export 命中 Canary | 默认派生特征、扫描、阻断发布 |
| 安装包覆盖上游应用 | P0 | Bundle/Data/Updater 相同 | 独立身份硬门禁 |
| 切模破坏缓存 | P0 Live | Cache Miss 飙升 | Cache Guard、Affinity、High Estimate |
| 复杂任务错误降档 | P0 Live | Unsafe Downgrade | Strong Recall Gate、Canary、自动回 Shadow |
| Provider 能力错误 | P0 Live | Tool/Streaming 失败 | 能力矩阵和 Probe |
| Hint 形成硬依赖 | P1 | Hint 离线导致异常 | Optional Adapter、TTL、Fail-open |
| Hint 绕过安全门禁 | P0 | Hint 强制 Candidate | Gate 优先级冻结 |
| 成本虚假精确 | P1 | Cache/Retry 缺失 | Low/Base/High + Assumptions |
| 上游迁移碰撞 | P0 | User Version 重用 | 当前上游 +1、真实库 Smoke |
| UI 与 Core 逻辑漂移 | P1 | UI 自己判断档位 | UI 只显示稳定 Code |
| HTTP 200 假成功 | P1 | Agent 实际失败 | 强/中/弱质量标签 |
| Retention 误删 Usage | P0 | Clear 范围不清 | AutoTier 表与基座 Usage 分开确认 |
| Writer Queue 内存增长 | P1 | DB 长期锁 | 有界 Queue、丢失计数、告警 |
| 上游同步破坏 Parity | P0 | Proxy 链改变 | 每次 Sync 重跑 Parity |

---

# 二十八、可复制验收场景

## 场景 A：AutoTier 单独使用

```text
前置：未安装 Potluck Web、Potluck Monitor，无 Hint
操作：添加 Provider → 配 Slot → Shadow 请求
期望：Decision、Usage、Cost Range 正常；无缺组件错误
```

## 场景 B：Off

```text
前置：已有 Provider，mode=off
操作：发送非流式、SSE、Tool 请求
期望：autotier_routing_decisions 零新增；出站与基线一致
```

## 场景 C：Model Mapping

```text
客户端请求：claude-sonnet-latest
基座映射：kimi-k2.5
期望：client=claude-sonnet-latest；baseline/actual=kimi-k2.5
```

## 场景 D：Failover

```text
初始 Provider A 返回 500，Backup B 成功
期望：initial=A；baseline/actual=B；usage provider=B
```

## 场景 E：DB Locked

```text
锁住 Decision Store
操作：发送请求
期望：客户端仍收到正常响应；记录观测丢失，不切模型
```

## 场景 F：Unparseable

```text
请求缺失合法 messages
期望：无 Candidate Slot；REQUEST_BODY_UNPARSEABLE；走基线
```

## 场景 G：隐私

```text
Prompt/API Key/Session 注入唯一 Canary Secret
操作：请求、错误、导出、崩溃路径
期望：DB/Log/Export 均搜索不到原文 Secret
```

## 场景 H：Hint 过期

```text
提供 expired PolicyHintV1
期望：忽略 Hint；Candidate 等于纯本地策略；请求正常
```

## 场景 I：Hint 与安全冲突

```text
Hint 建议 Cheap；请求含 Tool Error + 长上下文
期望：本地安全门禁胜出；Hint 不改变 safe_to_execute
```

## 场景 J：独立安装共存

```text
机器已有上游应用和数据库
操作：安装 AutoTier
期望：两个 Bundle 共存；AutoTier 新目录；不共写；导入需确认
```

## 场景 K：更新源

```text
操作：AutoTier 检查更新
期望：只访问 AutoTier Manifest；签名错误拒绝安装；不跳到上游
```

## 场景 L：清除

```text
操作：清除 AutoTier Decision/Label
期望：AutoTier 表对应数据删除；Provider Key 与基座 Usage 保留
```

---

# 二十九、每阶段固定交付模板

```text
Phase：
状态：
Commit：

1. 修改文件（总数必须 <=5）
2. 需求 ID / 本规格章节映射
3. 数据库迁移影响
4. 实现摘要
5. Compiler / Typecheck
6. Format / Lint
7. Unit / Integration / E2E
8. 手工验证
9. 隐私检查
10. 已知限制
11. 回滚方法
12. Exit Gate：PASS / FAIL
13. 工作树状态
14. 下一阶段建议

完成后停止，等待用户批准。
```

---

# 三十、Definition of Done

AutoTier v0.1 只有在以下全部成立时才算完成：

- 它是 AutoTier 独立安装包，而不是上游身份的开发构建；
- 没有 Potluck Monitor/Quota Snapshot/Policy Hint 时功能完整；
- Provider、Slot、Claude 接管流程完整；
- Off 与基座一致；
- Shadow 对合格请求稳定生成 Decision；
- 四组字段真实、无混用；
- 四 ID 无竞态关联；
- `autotier_mutated_request=false`；
- Actual Outbound = Baseline Outbound；
- Usage 和成本正确收口；
- Cache/Retry/Fallback 进入投影；
- Candidate Cost 不冒充真实节省；
- 用户可理解、标注、导出、清除；
- 默认无 Raw Prompt；
- Session Hash 有安装级 Salt；
- Replay 和 Session Holdout 可复现；
- Live Gate 报告真实展示“未达到/达到”，不自动开放 Live；
- 三端 Compiler、Lint、Test、Build、Install 全部通过；
- 独立 Bundle/Data/Updater/Signing 通过；
- 上游 Attribution 完整；
- 无 P0/P1 未解决问题；
- 文档、迁移、备份、回滚、已知限制和 Release Notes 完整；
- 当前在制 Phase 4 经过提交、验证和用户批准，不再是脏工作树草稿。

v0.1 完成不等于 Live 已完成。

Canary/Live 必须在真实数据达到门禁后另行立项。

---

# 三十一、最终裁决

1. AutoTier 继续作为独立模型路由产品实施，不并入 Potluck Monitor 核心。
2. v0.1 坚持 Measurement-first 和 Shadow-only。
3. Shadow 的唯一正确比较对象是基座出站结果，不是客户端请求值。
4. 四组字段、四 ID、隐私默认和 Provider-specific Slots 为冻结合同。
5. 当前 Phase 0–3.1 有提交和测试证据；Phase 4 只是未提交在制工作，必须继续审计和验证。
6. AutoTier 当前还不是可安全共存的独立安装包；Bundle ID、数据目录、更新源和接管所有权是发布硬门禁。
7. Cache、Retry、Fallback 和质量损失必须计入真实成本，不能用模型单价替代。
8. 外部 Policy Hint 只是未来可选输入；缺失、失效或删除时 AutoTier 必须完整运行。
9. Policy Hint 不得携带凭据，不得开 Live，不得绕过本地安全门禁。
10. 达到 v0.1 发布门禁后，先积累并评测真实 Shadow 数据；只有达到 Canary 数据门禁并得到用户明确批准，才开始下一份 Live PRD。

# AutoTier 产品需求文档（PRD）v1.0

> 文档类型：可执行产品需求、技术契约与交付门禁  
> 产品代号：AutoTier  
> 状态：Approved for Phase 0 / 禁止直接进入 Live Auto Routing  
> 版本：1.0  
> 日期：2026-07-29  
> 首个目标客户端：Claude Code  
> 产品形态：本地优先的 Tauri 桌面应用 + 本地 HTTP 代理  
> 权威原则：本 PRD 是实施合同；旧版 `autotier-architecture.md` 仅作背景材料，冲突时以本 PRD 为准。

---

## 0. 给执行 Agent 的总指令

### 0.1 当前总 Goal

在不改变用户任何真实上游模型选择的前提下，交付一个可安装、可关闭、隐私安全的 AutoTier v0.1：

1. 用户可以配置第三方 API Provider 并发现可用模型。
2. 用户可以为 Provider 配置 Cheap、Mid、Strong 三个逻辑槽位。
3. AutoTier 对每个 Claude Code 请求生成可解释的 Shadow 路由决策。
4. Shadow 模式绝不改写请求模型、Provider、协议或认证信息。
5. 系统准确记录原模型、候选槽位、候选模型、最终模型、理由、token、缓存和成本。
6. 用户能看到“理论可优化成本区间”，而不是未经验证的“已节省金额”。
7. 系统能导出回放数据，并按 Session 隔离执行离线评测。
8. 只有达到本文规定的质量、成本、隐私和工程门禁，后续版本才允许进入 10% Live Canary。

### 0.2 执行纪律

- 先完成 Phase 0，输出基座比较结论并等待明确批准；不得预先写业务代码。
- 每个实施 Phase 最多修改 5 个文件。
- 一次只执行一个 Phase。完成后必须运行该 Phase 的全部验证并提交证据。
- 任何 Phase 未通过门禁，不得进入下一 Phase。
- 不得把旧架构文档里的“省 85%”“70% 简单请求”“94.8% 准确率”写入产品文案。
- 不得默认保存原始 Prompt、System Prompt、Tool Result 或 API Key。
- 不得先实现 Live Auto Routing 再补 Shadow、评测或回滚。
- 不得用数据库保存完整会话状态替代请求内/内存状态。
- 不得将全局模型 ID 直接绑定到 Cheap/Mid/Strong；必须按 Provider 配置槽位。
- 关闭 AutoTier 路由功能时，代理行为必须与选定基座完全一致。
- 遇到需求歧义时，以“保持原模型、不改变请求、保护隐私、可回滚”为默认策略。

### 0.3 Agent 每阶段固定输出

每个 Phase 结束时必须输出：

1. 修改文件清单。
2. 需求 ID 与实现映射。
3. 数据库迁移影响。
4. 类型检查、Lint、单元测试、集成测试结果。
5. 手工验证路径和实际结果。
6. 已知限制。
7. 回滚方法。
8. 是否满足 Exit Gate。
9. 下一 Phase 建议，但不得自行开始。

---

# 1. 执行摘要

## 1.1 产品一句话

AutoTier 是面向多模型第三方 API 用户的本地 Agent 成本决策器：先观察每次请求是否真的需要昂贵模型，再以可解释、可回退的方式逐步帮助用户降低成本。

## 1.2 用户问题

目标用户通常拥有一个第三方 API Base URL 和 API Key。该 Provider 下有多个模型，但用户：

- 不知道模型能力差异；
- 不知道不同任务应使用哪个模型；
- 害怕便宜模型做坏复杂任务；
- 又不愿所有请求都使用昂贵模型；
- 不会编写路由规则；
- 无法准确计算缓存、重试和模型切换后的真实成本；
- 需要一个本地、低配置、可随时关闭的工具。

## 1.3 产品答案

AutoTier 不在第一天替用户做高风险自动切换，而是分三步建立信任：

1. **看见**：Shadow 分析请求，但保持用户原模型。
2. **证明**：用真实流量、Session Holdout 和成本模型证明哪些请求可以安全降档。
3. **启用**：只对高置信、低风险规则进行渐进式 Live Canary。

## 1.4 v0.1 的真实定位

v0.1 是“可观测路由与成本实验台”，不是“全自动模型路由器”。

v0.1 对外承诺：

- 本地处理；
- 不改真实模型；
- 每次建议可解释；
- 估算口径透明；
- 用户可清除数据；
- 为后续安全自动化提供证据。

v0.1 不承诺：

- 固定节省比例；
- 对所有请求分类正确；
- 自动选择全球最优模型；
- 支持所有 Agent；
- 在 Shadow 阶段产生真实节省。

---

# 2. 背景与已验证事实

## 2.1 已有基础

- cc-switch/相关 fork 已具备 Tauri 桌面壳、本地代理、Provider 管理、协议转换、熔断、Failover、SQLite、模型定价和用量统计。
- ludex 已具备可参考的复杂度信号、Open Slots、决策解释、Session 趋势、缓存 TTL、Failure Cooldown、离线评测和阈值拟合工具。
- 本机 cc-switch 数据库已存在 Provider、模型价格、请求日志和用量汇总。

## 2.2 已验证的风险

- ludex 的 115 条数据是单轮 Prompt，不含真实 Agent Tools、System Prompt 和 Messages History。
- ludex 的 94.8% 调优结果在同一数据集上拟合并评估，不能作为泛化准确率。
- Claude Code 请求中 Tools/System/History 可能恒真、近似常量或快速饱和。
- 模型切换会影响 Prompt Cache，单纯比较模型输入单价会高估节省。
- HTTP 200 不等于 Agent 任务成功。
- 全局三模型字符串无法适配不同 Provider 的模型 ID 和 Failover。
- 只扩展日汇总表无法记录逐请求的 Score、Reason 和 Shadow/Live 决策。

## 2.3 本 PRD 的关键修正

- Shadow First。
- Provider-specific Slots。
- 每请求 Routing Decision。
- Session-based Holdout。
- Cache/Retry-aware Cost。
- Privacy by Default。
- Live Routing 作为后续受门禁 Goal。

---

# 3. 产品愿景、目标与非目标

## 3.1 产品愿景

让不会写路由逻辑的 Agent 用户，也能像使用自动挡一样管理多模型成本，同时保留对质量、隐私和最终选择的控制。

## 3.2 北极星 Goal

在不显著降低任务成功率的前提下，帮助目标用户减少可避免的模型成本，并让每一次优化都有可复现证据。

## 3.3 v0.1 Goal

### G0：安全观测闭环

用户完成 Provider 配置后，AutoTier 能在 Shadow 模式下为 ≥99.9% 的合格请求生成决策记录，且不改变真实请求路径。

### G1：可解释决策

每条决策必须回答：

- 原请求使用什么模型；
- 系统建议哪个 Slot/模型；
- 为什么；
- 置信度多少；
- 为什么没有执行；
- 理论成本差是多少；
- 成本估算缺少哪些信息。

### G2：可信成本模型

成本投影分别计算：

- Input；
- Output；
- Cache Read；
- 5m Cache Write；
- 1h Cache Write；
- Retry；
- Fallback。

### G3：可复现评测

用户可导出匿名化特征和决策；评测必须按 Session 划分训练集与 Holdout，不允许同一 Session 泄漏。

### G4：隐私与可回退

- 默认不持久化原始 Prompt。
- 用户可一键清除所有 Routing Decision。
- Shadow 开关关闭后不再写入新决策。
- AutoTier 出错时保持原模型和原 Provider。

## 3.4 v0.2 Goal

仅在 v0.1 数据达到门禁后：

- 启用 Explicit-only Live Routing；
- 启用 10% 高置信 Canary；
- 失败一轮内自动升级 Strong；
- 展示真实已节省成本和质量影响。

## 3.5 非目标

- 企业 RBAC、审计审批和多租户。
- 云端托管网关。
- 自建模型推理。
- 代售 API Token。
- 默认上传 Prompt。
- 首版支持 Cursor、Cline、Codex、OpenCode 全部客户端。
- 使用 ML/Embedding/LLM 分类器。
- 自动在线学习。
- 自动改变用户的强制模型选择。
- 与 Provider 账单系统做财务级对账。

---

# 4. 目标用户与 Jobs To Be Done

## 4.1 核心用户

### Persona A：第三方 API 进阶小白

- 已能运行 Claude Code。
- 有一个 Base URL 和 API Key。
- Provider 下至少有两个模型。
- 每月模型预算约 30–300 美元。
- 不懂路由、缓存和协议。
- 需要低配置和可解释建议。

### Persona B：多模型重度个人用户

- 会手动切换 Kimi、DeepSeek、GLM、Claude、GPT 等模型。
- 已感到手动切换疲劳。
- 关心质量回退和任务失败成本。
- 愿意查看高级面板，但不愿维护复杂网关。

### Persona C：开发验证者

- 愿意参与 Shadow Dogfood。
- 能标注部分请求应为 Cheap/Mid/Strong。
- 需要导出决策和回放报告。

## 4.2 非目标用户

- 只有一个不可替换模型的用户。
- 完全不使用 API 的订阅用户。
- 已有成熟自建 LiteLLM/RouteLLM 平台的团队。
- 要求企业权限、集中结算或合规审计的组织。

## 4.3 JTBD

### JTBD-01：快速接入

当我拿到第三方 URL 和 Key 时，我希望快速验证并发现模型，而不是手写多份配置。

### JTBD-02：理解成本

当 Agent 连续工作时，我希望知道哪些请求可能用了过强模型，以及成本差来自哪里。

### JTBD-03：建立信任

在允许工具自动切模型之前，我希望先看到它在真实流量上的建议和证据。

### JTBD-04：安全回退

如果便宜模型不适合当前任务，我希望系统不影响工作，或者能立即恢复到原模型。

### JTBD-05：掌握控制权

我希望随时关闭、锁定档位、绕过某个 Session，并删除本地决策数据。

---

# 5. 产品原则

1. **原模型优先**：不确定时保持原模型。
2. **Shadow 优先**：先证明，再执行。
3. **质量优先于节省**：漏掉一次节省可以接受，错误降档复杂任务不可接受。
4. **Provider-aware**：Slot 是逻辑能力档，不是全局模型名。
5. **Cache-aware**：模型单价差不等于净节省。
6. **可解释**：每条建议必须有稳定 Reason Code。
7. **隐私默认**：默认只保存派生特征。
8. **可关闭**：关闭后恢复基座原行为。
9. **可复现**：任何指标都能从导出数据重新计算。
10. **不夸大**：Shadow 阶段只展示理论投影，不显示“已节省”。

---

# 6. 核心用户流程

## 6.1 首次接入

1. 用户打开 AutoTier。
2. 选择“添加 Provider”。
3. 输入 Provider 名称、Base URL、API Key。
4. 点击“测试连接”。
5. 系统调用兼容的模型发现接口；失败时提供手动模型录入。
6. 系统展示发现的模型，不自动假设能力。
7. 用户为 Cheap、Mid、Strong 各选择一个模型。
8. 系统验证三个模型可用性、协议和工具能力。
9. 用户点击“接入 Claude Code”。
10. 系统展示将修改的配置，用户确认后执行。
11. 默认开启 Shadow，而不是 Live。
12. 用户完成一条测试请求并查看决策卡。

## 6.2 Shadow 使用

1. Claude Code 请求进入本地代理。
2. AutoTier保存 Original Model。
3. 提取隐私安全特征。
4. 生成 Candidate Slot/Model。
5. Policy Gate 检测当前为 Shadow。
6. 请求保持 Original Model 和 Original Provider。
7. 上游响应返回。
8. Usage 与 Decision 通过 Request ID 关联。
9. UI 展示候选、理由、成本区间和“不曾执行”标记。

## 6.3 用户标注

1. 用户打开某条决策。
2. 选择：
   - 建议正确；
   - 应更强；
   - 可以更便宜；
   - 无法判断。
3. 可选填写简短原因。
4. 默认不要求用户提供原始 Prompt。
5. 标注进入本地评测数据。

## 6.4 导出回放

1. 用户选择日期范围。
2. 系统显示导出字段和隐私说明。
3. 用户选择：
   - 仅派生特征；
   - 派生特征 + 用户标注；
   - 原文（高风险、显式二次确认）。
4. 系统生成 JSONL 和元数据。
5. 回放工具输出默认策略、候选策略和 Baseline 对比。

## 6.5 关闭与清理

1. 用户关闭 Shadow。
2. 新请求不再运行分类器、不写决策。
3. Proxy 继续使用基座默认逻辑，或用户选择完全停止 Proxy。
4. 用户可清除：
   - 决策记录；
   - 用户标注；
   - 导出文件历史；
   - 所有 AutoTier 配置。

---

# 7. 功能需求

## 7.1 Provider 与模型槽位

### FR-PROV-001：Provider 配置

优先级：P0

系统必须支持：

- 名称；
- Base URL；
- API Key；
- API Format；
- 可选 Headers；
- 连接测试；
- Key 安全存储；
- 编辑与删除。

验收：

- 无效 URL 不保存。
- Key 不以明文出现在日志和 UI。
- 测试失败给出可操作错误。
- 删除前提示其 Slot 和 Claude 接入依赖。

### FR-PROV-002：模型发现

优先级：P0

- 优先调用 Provider 的模型列表接口。
- 失败时允许手动输入。
- 发现结果记录来源和时间。
- 不根据模型名直接宣称质量。

验收：

- 超时、401、403、404、非标准响应分别处理。
- 模型列表为空时仍能手动配置。
- 刷新不会删除用户手动映射。

### FR-SLOT-001：Provider-specific Slots

优先级：P0

每个 Provider 独立配置：

- Cheap；
- Mid；
- Strong；
- 可选 Long Context；
- 可选 Background。

验收：

- Cheap/Mid/Strong 均必填才能启用 Shadow 候选模型。
- 同一模型可被多个 Slot 引用，但 UI 提示。
- 模型被 Provider 删除后，Slot 标记无效。
- Failover Provider 必须有自己的 Slot 映射。

### FR-SLOT-002：能力验证

优先级：P0

模型槽位必须记录或验证：

- API Format；
- Tool Use；
- Streaming；
- Context Limit；
- Vision；
- Input/Output/Cache Pricing；
- 数据来源；
- 最近验证时间。

验收：

- 不支持 Tool Use 的模型不能用于带工具的 Live Candidate。
- 缺价格时允许 Shadow，但成本显示“不完整”。
- 能力未知时 Safe-to-execute 必须为 false。

---

## 7.2 模式与控制

### FR-MODE-001：路由模式

优先级：P0

完整产品生命周期规划以下模式：

- Off；
- Shadow；
- Explicit-only；
- Canary-live；
- Full-live；
- Forced-cheap；
- Forced-mid；
- Forced-strong。

其中 Explicit-only、Canary-live、Full-live、Forced-cheap、Forced-mid、Forced-strong 均为 v0.2+ 候选能力，不属于 v0.1 实现范围。

v0.1 只实现并开放：

- Off；
- Shadow。

Shadow 页面可以提供 Forced Candidate Slot 调试控件；它是候选决策覆盖参数，不是独立路由模式，仅覆盖 Shadow 推荐候选，不改写 Final Model/Provider。

验收：

- 默认 Shadow。
- 模式变更有本地审计记录。
- Off 模式不运行特征提取，不写 Decision。
- Shadow 模式 Candidate 与 Final 必须可区分。
- v0.1 的 Forced Candidate Slot 只改变 `recommended_slot`/`candidate_model`，仍必须满足 `final_model == original_model` 与 `final_provider == original_provider`。
- v0.1 不得通过 UI、配置文件、Header 或隐藏 Feature Flag 暴露真实 Forced-live 行为。

### FR-MODE-002：Session Bypass

优先级：P1

- 用户可对当前 Session 关闭建议。
- 请求 Header/Metadata 可显式绕过。
- Bypass 原因写入决策或操作日志。

### FR-MODE-003：故障安全

优先级：P0

任何以下情况保持原模型：

- 分类器异常；
- 配置缺失；
- Slot 无效；
- Provider 无候选模型；
- 能力未知；
- Cost 模型不完整且策略要求 Cost Gate；
- Policy 版本不兼容；
- Request Body 无法安全解析。

---

## 7.3 Shadow 决策

### FR-DEC-001：特征提取

优先级：P0

默认允许持久化的派生特征：

- App Type；
- Original Model；
- User Message 加权长度；
- Message Count Bucket；
- User Turn Count Bucket；
- Tool Definition Count；
- Tool Result Count；
- 是否包含 Error Tool Result；
- Constraint Count；
- Code Structure Score；
- Image/File 标记；
- Context Token Bucket；
- Cache Read/Write Token；
- Effort/Thinking 标记；
- Recent Complexity Window；
- Session ID Hash；
- Feature Version。

禁止默认持久化：

- 原始 User Message；
- System Prompt；
- Tool Schema 全文；
- Tool Result 全文；
- 文件内容；
- API Key；
- Authorization Header；
- 完整 Session ID。

### FR-DEC-002：决策输出

优先级：P0

每次分类输出：

```text
recommended_slot
candidate_model
complexity_score
confidence
reason_codes[]
safe_to_execute
unsafe_reasons[]
feature_version
classifier_version
policy_version
```

验收：

- Reason Code 使用稳定枚举。
- UI 不解析自由文本决定逻辑。
- 同一 Feature/Classifier/Policy 版本对相同输入产生确定性结果。

### FR-DEC-003：原模型不变

优先级：P0，阻断级

Shadow 模式必须满足：

```text
final_model == original_model
final_provider == original_provider
```

验收：

- 单元测试覆盖。
- 集成测试覆盖。
- 真实 Proxy 请求抓包验证。
- 任何不一致均阻断发布。

### FR-DEC-004：决策解释

优先级：P0

UI 必须以用户语言解释：

- 为什么建议该 Slot；
- 哪些因素提高复杂度；
- 哪些风险阻止执行；
- 为什么成本估算是区间；
- 该建议从未影响真实请求。

禁止展示：

- “AI 判断你只需要弱模型”；
- “保证节省 X%”；
- 不可复现的黑箱分数。

---

## 7.4 决策日志与数据

### FR-DATA-001：Routing Decision 持久化

优先级：P0

每条 Decision 通过 Request ID 与 Usage 关联。

推荐字段见第 11 节。

验收：

- 请求成功、失败、流式中断都能收口。
- 没有 Usage 时 Decision 标记 incomplete。
- 同一 Request ID 幂等写入。
- 不将 Candidate Cost 当 Actual Cost。

### FR-DATA-002：保留与清除

优先级：P0

- 默认保留 30 天。
- 用户可配置 0、7、30、90 天。
- 0 天表示仅内存、不持久化。
- 清除操作立即生效。
- 清除不删除原 cc-switch 用量数据，除非用户另行选择。

### FR-DATA-003：导出

优先级：P0

导出格式：

- `decisions.jsonl`；
- `labels.jsonl`；
- `manifest.json`。

Manifest 包含：

- Schema Version；
- Feature Version；
- Classifier Version；
- Policy Version；
- 导出时间；
- 过滤范围；
- 是否包含原文；
- Hash Salt Scope；
- 价格快照来源。

---

## 7.5 成本与报告

### FR-COST-001：实际成本

优先级：P0

实际成本按最终模型和实际 Usage 计算：

```text
input_cost
+ output_cost
+ cache_read_cost
+ cache_write_5m_cost
+ cache_write_1h_cost
+ retry_cost
+ fallback_cost
```

### FR-COST-002：候选成本

优先级：P0

Shadow Candidate 成本必须显示为投影，并包含：

- Low Estimate；
- Base Estimate；
- High Estimate；
- 缺失因素；
- 价格时间戳；
- 是否假设缓存继续命中；
- 是否包含失败重试。

### FR-COST-003：报告文案

优先级：P0

Shadow 阶段使用：

- “理论可优化成本”；
- “候选模型成本投影”；
- “未实际执行”；
- “不包含/包含的成本因素”。

禁止：

- “已节省”；
- “为你省了”；
- “装上就省 85%”；
- 将全用最贵模型作为唯一默认 Baseline。

### FR-COST-004：Baseline

优先级：P0

用户可选择：

- Actual Original Model；
- User-selected Strong Slot；
- All-Strong Scenario；
- Custom Baseline。

默认使用 Actual Original Model。

---

## 7.6 用户标注与评测

### FR-LABEL-001：决策标注

优先级：P0

标注枚举：

- Correct；
- Should Be Stronger；
- Could Be Cheaper；
- Unsure。

可选原因：

- Tool Failure Risk；
- Long Context；
- Architecture/Reasoning；
- Simple Formatting；
- Background Task；
- Wrong Provider Capability；
- Other。

### FR-EVAL-001：Session Holdout

优先级：P0

- 必须按 Session Hash 分组切分。
- 默认 70% Train/Tune、30% Holdout。
- Holdout 只用于最终评估。
- 同一 Session 不得跨集合。
- 报告同时显示样本量和类别分布。

### FR-EVAL-002：评测指标

优先级：P0

至少输出：

- Strong Recall；
- Unsafe Downgrade Rate；
- Cheap Precision；
- Overall Accuracy；
- Strong Ratio；
- Projected Net Saving；
- Cache-adjusted Saving；
- Per-rule Distribution；
- Confidence Calibration；
- Unknown/Bypass Ratio。

### FR-EVAL-003：对照组

优先级：P0

必须包含：

- Original Routing；
- All-Strong；
- All-Cheap；
- Random；
- Default Policy；
- Tuned Policy；
- Explicit-only。

禁止只报告最优策略。

---

# 8. UX 信息架构

## 8.1 页面

### 首页

- Proxy 状态；
- 当前 Provider；
- 当前模式；
- 今日请求数；
- Shadow 建议分布；
- 理论优化区间；
- 数据完整性提示；
- 最近 5 条决策。

### Provider

- Provider 列表；
- 连接状态；
- Base URL；
- 模型刷新；
- Slot 配置；
- 能力/价格来源；
- 最近验证时间。

### 智能路由

- Off/Shadow；
- Forced Candidate Slot（可选调试控件，不执行真实路由）；
- 高级阈值只读或开发开关；
- 隐私设置；
- 保留时间；
- Bypass；
- Feature/Policy 版本。

### 决策日志

筛选：

- 日期；
- Session；
- Original Model；
- Candidate Slot；
- Reason；
- Confidence；
- Label；
- Complete/Incomplete。

单条详情：

- Original/Candidate/Final；
- 特征摘要；
- 决策理由；
- 成本投影；
- Actual Usage；
- 用户标注；
- Shadow 未执行标记。

### 评测

- 数据量；
- Session 数；
- 标注覆盖；
- Holdout 指标；
- 规则分布；
- 是否达到 Live Gate；
- 导出与回放。

### 隐私与数据

- 保存哪些字段；
- 不保存哪些字段；
- 保留期限；
- 一键清除；
- 原文采集显式开关；
- 导出预览。

## 8.2 首次使用文案

必须明确：

> AutoTier 当前运行在 Shadow 模式。它会分析并记录建议，但不会替换你真实使用的模型。积累足够数据后，你可以查看评测结果，再决定是否允许自动路由。

## 8.3 错误文案原则

错误必须包含：

- 发生了什么；
- 是否影响真实请求；
- 已采取的安全回退；
- 用户下一步。

例如：

> 无法验证 Cheap 槽位模型的工具调用能力。本次请求保持原模型，未执行降档。请在 Provider 设置中重新测试该模型。

---

# 9. 系统架构

```text
Claude Code
  │
  ▼
Local Proxy Handler
  │
  ├─ Request ID / Session Hash
  ├─ Original Model / Provider
  ▼
Feature Extractor
  │
  ├─ Privacy-safe Features
  └─ Feature Version
  ▼
Decision Engine
  │
  ├─ Recommended Slot
  ├─ Score / Confidence
  ├─ Reason Codes
  └─ Safe-to-execute
  ▼
Policy Gate
  │
  ├─ Off: no decision
  ├─ Shadow: record only
  ├─ Explicit-only: deterministic mapping
  └─ Canary/Live: future, gated
  ▼
Provider-aware Slot Resolver
  │
  ├─ Provider Model Availability
  ├─ Capability Validation
  ├─ Protocol Compatibility
  └─ Cache / Cost Guard
  ▼
Existing Execution Pipeline
  │
  ├─ Provider Routing
  ├─ Model Mapping
  ├─ Protocol Transform
  ├─ Circuit Breaker / Failover
  └─ Forward / SSE
  ▼
Usage Collector
  │
  ├─ Tokens
  ├─ Cache
  ├─ Cost
  ├─ Latency
  └─ Outcome
  ▼
Routing Decision Finalizer
  │
  └─ Decision + Usage via Request ID
  ▼
SQLite → Dashboard / Export / Replay / Eval
```

## 9.1 模块边界

### Feature Extractor

纯函数；不得访问数据库；不得做 Provider 选择。

### Decision Engine

输入特征，输出候选决策和 Next State；不得直接改写请求。

### Policy Gate

唯一有权决定 Candidate 是否提交为 Final。

### Slot Resolver

唯一有权把逻辑 Slot 解析为 Provider Model。

### Execution Pipeline

尽量复用基座；不在其中重复实现复杂度逻辑。

### Decision Store

只存决策和派生数据；不负责 Session 运行状态。

---

# 10. 路由状态机

```text
OFF
  └─ 不提取、不记录、不改写

SHADOW
  ├─ 提取特征
  ├─ 生成 Candidate
  ├─ Final = Original
  └─ 写 Decision

EXPLICIT_ONLY（v0.2）
  ├─ 客户端显式 model/metadata 映射
  ├─ 不使用启发式降档
  └─ 失败保持 Original

CANARY_LIVE（v0.2）
  ├─ 仅 allowlist 规则
  ├─ 高置信
  ├─ Provider/Capability/Cache Gate
  ├─ 按比例启用
  └─ 一轮失败升级 Original/Strong

FULL_LIVE（未来）
  └─ 只有长期证据后开放
```

允许的状态迁移：

```text
Off ↔ Shadow
Shadow → Explicit-only
Explicit-only → Canary-live
Canary-live → Full-live
任意状态 → Off
任意 Live 状态发生安全事件 → Shadow
```

禁止：

```text
首次安装 → Canary-live
首次安装 → Full-live
Shadow 数据不足 → Live
配置无效 → Live
```

---

# 11. 数据模型

以下为逻辑 Schema；Phase 0 必须根据选定基座的实际迁移系统调整命名。

## 11.1 `autotier_provider_slots`

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

## 11.2 `autotier_routing_config`

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

## 11.3 `autotier_routing_decisions`

```sql
CREATE TABLE autotier_routing_decisions (
  request_id TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL,
  completed_at INTEGER,
  app_type TEXT NOT NULL,
  session_hash TEXT,
  mode TEXT NOT NULL,

  original_provider_id TEXT,
  original_model TEXT NOT NULL,
  recommended_slot TEXT,
  candidate_provider_id TEXT,
  candidate_model TEXT,
  final_provider_id TEXT,
  final_model TEXT,

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

## 11.4 `autotier_decision_labels`

```sql
CREATE TABLE autotier_decision_labels (
  request_id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  reason TEXT,
  note TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (request_id)
    REFERENCES autotier_routing_decisions(request_id)
    ON DELETE CASCADE
);
```

## 11.5 索引

至少需要：

- created_at；
- session_hash；
- original_model；
- recommended_slot；
- classifier_version；
- label；
- is_complete。

## 11.6 数据迁移原则

- 迁移必须幂等。
- 新表使用 `autotier_` 前缀，避免污染上游概念。
- 不修改现有 Usage 主键。
- 通过 Request ID 关联，不复制所有 Usage 数据。
- 卸载/关闭功能不删除原 cc-switch 数据。

---

# 12. 决策引擎契约

## 12.1 输入

```rust
pub struct DecisionInput {
    pub request_id: String,
    pub app_type: AppType,
    pub original_model: String,
    pub provider_id: Option<String>,
    pub features: RoutingFeatures,
    pub session_state: RoutingSessionState,
    pub mode: RoutingMode,
    pub feature_version: String,
}
```

## 12.2 输出

```rust
pub struct DecisionResult {
    pub recommended_slot: Option<ModelSlot>,
    pub complexity_score: f32,
    pub confidence: f32,
    pub reason_codes: Vec<ReasonCode>,
    pub safe_to_execute: bool,
    pub unsafe_reasons: Vec<UnsafeReason>,
    pub next_state: RoutingSessionState,
    pub classifier_version: String,
    pub policy_version: String,
}
```

## 12.3 纯度

- `decide(input)` 不修改传入状态。
- 返回 `next_state`。
- Shadow Store 与 Decision Engine 分离。
- Clock 通过参数注入，测试不得依赖真实时间。
- 相同版本和相同输入必须产生相同输出。

## 12.4 Reason Code 初始枚举

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

## 12.5 v0.1 默认策略

v0.1 可以计算推荐，但 `safe_to_execute` 默认 false。

只有以下显式情况可标为未来可执行候选：

- 用户强制 Slot；
- 客户端明确 Background Metadata；
- 客户端显式 Small/Fast Alias；
- Provider 和模型能力已验证；
- 不存在 Tool Error、Long Context、未知能力。

即便如此，v0.1 Shadow 仍不执行。

---

# 13. 成本模型

## 13.1 实际成本

```text
actual =
  input_tokens              × input_price
+ output_tokens             × output_price
+ cache_read_tokens         × cache_read_price
+ cache_write_5m_tokens     × cache_write_5m_price
+ cache_write_1h_tokens     × cache_write_1h_price
+ retries
+ fallbacks
```

## 13.2 Candidate 投影

候选模型的输出 Token、重试概率和缓存命中未知，必须使用区间：

```text
Low:
  假设输出 Token 不增加、缓存继续命中、无重试

Base:
  使用同类历史模型的输出倍率、缓存折损中位数、观察到的重试率

High:
  假设缓存失效、输出增长、发生一次失败重试
```

## 13.3 净优化

```text
projected_net_saving = baseline_actual_cost - candidate_projected_cost
```

如果 Candidate 价格缺失：

- 不显示金额；
- 仍显示模型和理由；
- 标记 `COST_DATA_INCOMPLETE`。

## 13.4 价格快照

Decision 必须引用发生当时的价格来源和时间，避免之后价格更新改变历史报告。

---

# 14. 隐私、安全与合规

## 14.1 本地优先

- 决策与存储默认只在本机。
- v0.1 不提供云端遥测。
- 不要求创建 AutoTier 云账号。

## 14.2 凭据

- API Key 使用基座现有安全存储。
- 日志统一脱敏。
- Error 不得包含 Authorization。
- 导出不得包含 Key。

## 14.3 Prompt

默认：

- 只在内存中读取；
- 只持久化派生特征；
- 不记录原文。

Raw Prompt Opt-in：

- 单独开关；
- 二次确认；
- 显示保存位置和保留期限；
- 可随时关闭和清除；
- 导出前再次提示。

## 14.4 Session Hash

- 不保存完整 Session ID。
- 使用本机 Salt。
- Salt 轮换后不可跨周期关联。
- 导出 Manifest 说明 Hash Scope。

## 14.5 日志

禁止写入：

- Request Body；
- Response Body；
- Headers；
- API Key；
- Raw Prompt；
- 文件内容。

允许：

- Request ID；
- Error Code；
- Reason Code；
- 模型 ID；
- Token/Cost；
- Latency；
- Schema/Policy Version。

---

# 15. 非功能需求

## NFR-001：性能

- Feature + Decision p95 <1ms。
- p99 <5ms。
- Shadow 不新增网络调用。
- 不阻塞 SSE 首 Token。

## NFR-002：可靠性

- 分类器异常不影响请求。
- Decision Store 写失败不影响请求。
- UI 不运行时 Proxy 可继续。
- 数据库锁冲突不得阻断上游转发。

## NFR-003：兼容性

- macOS、Windows、Linux 保持基座支持范围。
- Claude Code 的 Anthropic Messages 流式和非流式均验证。
- 关闭 AutoTier 后与基座行为一致。

## NFR-004：可维护性

- Feature、Classifier、Policy、Schema 独立版本化。
- Reason Code 使用枚举。
- 不在 UI 中复制分类逻辑。
- 不依赖模型名称猜测作为唯一能力来源。

## NFR-005：可测试性

- Clock、Session、Provider Capabilities 可注入。
- Feature Extractor 为纯函数。
- Decision Engine 为纯函数。
- Cost Calculator 为纯函数。
- 数据库使用临时隔离实例测试。

---

# 16. 验收指标与发布门禁

## 16.1 v0.1 发布门禁

### 功能

- Provider 配置和连接测试通过。
- Slot 配置通过。
- Shadow Decision Coverage ≥99.9%。
- Decision/Usage 关联成功率 ≥99%。
- Off 模式不产生 Decision。
- Shadow 模式 100% 保持 Original Model/Provider。
- 数据清除通过。
- 导出/回放通过。

### 隐私

- 默认数据库无 Raw Prompt。
- 日志扫描无 Key/Header/Body。
- 导出默认无 Raw Prompt。
- 清除后相关 Decision/Label 不可查询。

### 工程

- Strict Typecheck/Compiler 通过。
- 所有 Lint 通过。
- 全部单元测试通过。
- 全部集成测试通过。
- 基座原测试通过。
- 三端构建或 CI 通过。
- 真实 Claude Code 流式请求通过。
- 关闭功能行为对比通过。

## 16.2 Live Canary 数据门禁

满足全部条件后，才可新建 v0.2 Live Phase：

- ≥500 个目标请求；
- ≥50 个独立 Session；
- ≥200 条高质量用户标注或等价人工评审；
- Holdout Strong Recall ≥98%；
- Unsafe Downgrade ≤2%；
- Unknown/Bypass 有透明统计；
- Cache-adjusted Net Saving Base Estimate ≥15%；
- 决策延迟达标；
- 无 P0/P1 数据或安全缺陷；
- 用户明确 Opt-in；
- 一键回退 Shadow 已验证。

## 16.3 自动降级停止条件

任何以下情况触发 Live → Shadow：

- Unsafe Downgrade 超过 2%；
- Tool Failure 显著升高；
- Retry/Fallback 成本抵消节省；
- 数据关联失败率 >1%；
- Provider Slot 失效；
- 新 Classifier/Policy 未通过 Holdout；
- 用户投诉出现可复现质量下降。

---

# 17. 测试策略

## 17.1 单元测试

### Feature Extractor

- 中英文长度；
- 空消息；
- 多轮；
- Tool Definition；
- Tool Result Error；
- 代码块；
- 多模态；
- 超长输入；
- 非标准 Content Block；
- Raw Prompt 不进入持久化结构。

### Decision Engine

- 确定性；
- Clock 注入；
- Next State 不修改 Input；
- Reason Code；
- Unknown Capability；
- Forced Candidate Slot；
- Bypass；
- Classifier Error；
- Shadow Safe-to-execute 行为。

### Cost

- Input/Output；
- Cache Read；
- 5m/1h Write；
- Retry；
- Fallback；
- 缺价格；
- Decimal 精度；
- 价格快照。

### Schema/DAO

- Migration；
- 幂等；
- Request ID Upsert；
- Incomplete → Complete；
- Cascade Delete；
- Retention；
- 并发写。

## 17.2 集成测试

- Claude Non-streaming；
- Claude SSE；
- Tool Use；
- Tool Result；
- Provider Error；
- Decision Store Error；
- DB Locked；
- Proxy Restart；
- Off/Shadow 切换；
- 删除 Slot；
- Provider Refresh；
- Failover；
- Request ID 关联；
- 关闭后基座行为一致。

## 17.3 E2E

全新用户流程：

1. 安装；
2. 添加 Provider；
3. 发现模型；
4. 配置 Slot；
5. 接入 Claude Code；
6. 发简单请求；
7. 发工具任务；
8. 查看决策；
9. 标注；
10. 导出；
11. 清除；
12. 关闭。

## 17.4 回归

- 基座完整测试。
- 已有 Provider 协议转换。
- SSE。
- Usage。
- Circuit Breaker。
- Failover。
- Model Mapping。
- Updater/Tray/Config。

---

# 18. 实施计划

> 任何实际文件路径必须由 Phase 0 根据选定基座确认。以下路径是职责占位符，不授权 Agent 猜测路径后直接修改。

## Phase 0：基座选择与可行性 Spike

状态：下一步执行  
代码修改：禁止  
预计：1–2 天  

目标：

- 比较最新 `farion1231/cc-switch` 与 `BigStrongSun/ccswitchmulti`。
- 跑通各自构建、测试和 Claude 请求链。
- 找到实际 Handler、Forwarder、Usage、Migration、Settings 路径。
- 验证 Request ID 和 Original Model 在链路中的可用性。
- 验证 Claude 路径是否能跨 Provider 解析 Slot。

产物：

- `docs/autotier/base-selection.md`
- `docs/autotier/path-map.md`
- `docs/autotier/baseline-verification.md`

Exit Gate：

- 明确选定仓库和 Commit SHA。
- 构建/测试结果完整。
- 明确 Shadow 插入点。
- 明确 Usage Finalize 点。
- 明确迁移方式。
- 明确关闭功能的 Parity 验证方法。
- 用户批准后才进入 Phase 1。

## Phase 1：领域类型与配置契约

最多修改 5 个文件。

目标：

- 定义 Mode、Slot、Decision、Feature、Reason、Unsafe Reason。
- 不接入 Forwarder。
- 不写数据库。
- 所有类型和纯函数契约有测试。

Exit Gate：

- Strict Compiler。
- Lint。
- 类型测试。
- 无运行行为变化。

## Phase 2：数据库 Migration 与 DAO

最多修改 5 个文件。

目标：

- 新建 Config、Slots、Decisions、Labels。
- 实现幂等写、Finalize、Retention、Delete。
- 不接入真实请求。

Exit Gate：

- Migration 正反向或安全回滚验证。
- DAO 单测。
- 并发和 DB Lock 测试。
- 不影响现有 Usage。

## Phase 3：Feature Extractor 与 Decision Engine

最多修改 5 个文件。

目标：

- 纯 Feature Extractor。
- 纯 Decision Engine。
- 初始 Shadow Policy。
- Reason Code。
- Session Next State。

Exit Gate：

- 全部边界测试。
- 默认不产生 Raw Prompt 持久化字段。
- Bench p95 <1ms。
- 同输入确定性。

## Phase 4：Shadow Proxy 接入

最多修改 5 个文件。

目标：

- 在请求链保存 Original Model/Provider。
- 运行 Candidate Decision。
- Shadow 强制 Final = Original。
- 创建 Incomplete Decision。
- 不等待数据库写入阻塞请求。

Exit Gate：

- 抓包证明模型/Provider 未改变。
- Streaming/Non-streaming 通过。
- Decision Store 失败不影响请求。
- Off 模式行为一致。

## Phase 5：Usage Finalize 与成本计算

最多修改 5 个文件。

目标：

- 请求完成时关联 Usage。
- 处理流式中断和失败。
- 计算 Actual/Projected Range。
- 保留价格快照。

Exit Gate：

- Request ID 关联 ≥99% 测试样本。
- Cache/Retry/Fallback 单测。
- Candidate 不被记为 Actual。

## Phase 6：Provider Slot UI

最多修改 5 个文件。

目标：

- Off/Shadow Mode。
- Provider-specific Slots。
- Forced Candidate Slot 调试控件。
- Capability/Price Status。
- Shadow 解释。
- 隐私提示。

Exit Gate：

- 首次配置 E2E。
- 无效 Slot 阻止启用。
- UI 无 API Key 泄漏。
- 可访问性基础检查。

## Phase 7：决策日志、标注与清除

最多修改 5 个文件。

目标：

- 列表和详情。
- 标注。
- Retention。
- Clear。
- Export。

Exit Gate：

- 清除验证。
- 导出 Schema 验证。
- 默认导出无原文。

## Phase 8：Replay 与 Eval

最多修改 5 个文件。

目标：

- JSONL Replay。
- Session Split。
- Baselines。
- Holdout。
- Metrics。
- Live Gate 报告。

Exit Gate：

- 同一 Session 不跨集合。
- 结果可复现。
- Tuned 与 Holdout 分离。
- 样本量和分布透明。

## Phase 9：v0.1 发布验证

最多修改 5 个文件；如验证发现需要修改更多文件，必须拆出新的修复 Phase，完成验证并再次等待批准。

目标：

- 全新安装。
- 三端构建。
- 完整测试。
- 新用户 E2E。
- Dogfood。
- 隐私审计。
- 文案审计。

Exit Gate：

- 第 16.1 节全部满足。
- 无未解决 P0/P1。
- Release Notes 明确 Shadow。

## Phase 10：Live Canary（不属于 v0.1）

只有第 16.2 节全部满足后才允许规划，不得提前实现。

---

# 19. Goal Tree 与完成定义

## Goal A：证明基础链路可用

完成条件：

- 选定基座；
- Claude 请求链确认；
- Shadow 和 Usage 插入点确认；
- 关闭行为可验证。

## Goal B：建立安全观测

完成条件：

- Decision Coverage；
- Shadow Parity；
- Privacy；
- Decision/Usage 关联；
- Cost Range。

## Goal C：建立产品信任

完成条件：

- 用户能理解建议；
- 用户能标注；
- 用户能导出；
- 用户能清除；
- 报告不夸大。

## Goal D：建立质量证据

完成条件：

- 足够数据；
- Session Holdout；
- Strong Recall；
- Unsafe Downgrade；
- Cache-adjusted Saving。

## Goal E：允许有限自动化

完成条件：

- Goal A–D 全部完成；
- 用户 Opt-in；
- Canary；
- 自动回退；
- 停止条件。

## v0.1 Definition of Done

AutoTier v0.1 只有在以下全部成立时完成：

- 用户可以完成 Provider 和 Slot 配置。
- Claude Code 请求稳定经过本地代理。
- Shadow 生成决策。
- 原请求 100% 不变。
- Usage 能收口。
- 成本投影透明。
- 标注/导出/清除可用。
- 默认无 Raw Prompt。
- 回放和 Holdout 可复现。
- 三端和全套测试通过。
- 文案没有未经验证的百分比。
- 文档、迁移、回滚和已知限制完整。

---

# 20. 决策记录

## ADR-001：本地代理而非 MCP-first

理由：确定性拦截、协议/用量/缓存可观察。

## ADR-002：Shadow-first

理由：分类质量和成本收益尚未在目标流量验证。

## ADR-003：Provider-specific Slots

理由：模型 ID、能力、价格和 Availability 随 Provider 不同。

## ADR-004：运行状态不直接持久化为完整 Session

理由：避免并发、隐私和 stale state；决策日志与运行状态分离。

## ADR-005：按 Session Holdout

理由：防止同一会话历史泄漏导致虚高准确率。

## ADR-006：投影区间而非单一节省值

理由：输出长度、缓存和失败重试是未知变量。

## ADR-007：基座在 Phase 0 决定

理由：官方上游和 ccswitchmulti 均快速演进，旧调研不足以锁定当前主干。

---

# 21. 风险登记

## R-001：复杂任务错误降档

严重度：P0  
缓解：Shadow、Strong Recall Gate、Unknown 保持原模型。

## R-002：切模型破坏缓存

严重度：P0  
缓解：Cache Guard、成本区间、Session Stickiness 后置。

## R-003：Provider 模型不存在

严重度：P0  
缓解：Provider Slots、启动验证、Original Fallback。

## R-004：Prompt 隐私泄漏

严重度：P0  
缓解：派生特征、日志脱敏、显式 Opt-in、清除。

## R-005：报告虚假精确

严重度：P1  
缓解：区间、假设披露、Baseline 可选。

## R-006：基座升级冲突

严重度：P1  
缓解：模块边界、上游优先、Commit 锁定、小文件 Phase。

## R-007：决策日志阻塞代理

严重度：P0  
缓解：非阻塞写、失败不影响请求、Incomplete 收口。

## R-008：HTTP 200 质量失败

严重度：P1  
缓解：Outcome/Retry/Tool Error 信号，Live 前验证。

---

# 22. 开发 Agent 启动 Prompt

将以下内容与本 PRD 一起交给执行 Agent：

```text
你正在实施 AutoTier。`AutoTier-PRD-v1.0.md` 是唯一权威产品与技术合同。

当前只执行 Phase 0：基座选择与可行性 Spike。

规则：
1. 不写 AutoTier 业务代码。
2. 分别检查最新 farion1231/cc-switch 与 BigStrongSun/ccswitchmulti。
3. 锁定各自 commit SHA。
4. 运行各自配置的编译、类型检查、Lint 和测试。
5. 追踪 Claude Code 请求从 Handler 到 Forwarder、Provider Resolver、Model Mapper、Protocol Transform、Usage Finalize 的完整路径。
6. 确认原始 model/provider/request_id/session_id 在每个阶段是否可用。
7. 确认 Shadow Decision 最小侵入插入点。
8. 确认 Provider-specific Slot 解析需要修改哪些模块。
9. 确认关闭 AutoTier 后如何证明行为与基座一致。
10. 输出：
   - docs/autotier/base-selection.md
   - docs/autotier/path-map.md
   - docs/autotier/baseline-verification.md
11. 不得开始 Phase 1。

选择基座时优先考虑：
- Claude Code-first 路径正确性；
- 最新 upstream 同步成本；
- 现有测试和三端构建；
- Provider-aware model routing；
- Usage/Request ID 关联；
- 协议转换和 Streaming 稳定性；
- 最小长期维护面。

完成后报告 Exit Gate 是否满足，并等待用户批准。
```

---

# 23. 最终批准结论

本 PRD 批准：

- Phase 0 基座选择；
- v0.1 Shadow 产品；
- Provider-specific Slots；
- 隐私安全特征；
- Routing Decisions；
- 回放与 Session Holdout；
- 成本区间报告。

本 PRD 不批准：

- 直接实现 Full-live；
- 直接移植旧六维分类器；
- 默认宣称省 85%；
- 使用 ludex 94.8% 作为产品指标；
- 未比较最新基座就永久 fork；
- 默认保存 Raw Prompt；
- 把 Candidate Cost 当 Actual Saving。

执行起点：Phase 0。  
Phase 0 完成并获明确批准后，才能进入 Phase 1。

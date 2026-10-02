# Session-aware Shadow 设计说明

## 目的

AutoTier 的 v0.1 仍然是 Shadow-only：它只记录特征、候选和决策解释，不执行模型切换。此次优化把从上游代理项目学到的原则落到 Shadow 链路，并经过一轮独立对抗审查（review）与同类项目研究（cc-switch upstream、claude-code-router、CLIProxyAPI）修正了第一版的三个语义错误。

## 实现边界

- 会话 key 是 `(app_type, provider_id, HMAC(session_id))`。原始 Session ID 只在请求处理的内存路径中短暂使用，不进入数据库或导出文件。
  - `provider_id` 在 key 中的原因：缓存连续性是 Provider 级属性——failover 或用户切换 Provider 后，上游 KV cache 不复存在，旧槽位推荐不能作为保护依据。与基座 `GeminiShadowKey(provider_id, session_id)` 的先例一致。
- **只为真实会话建状态**：`session_client_provided == false`（提取失败兜底生成随机 UUID）的请求不写入 store，只用默认状态做单次无状态观察。否则每个匿名请求产生一条永不命中的孤立条目，LRU 会被碎片填满。CLIProxyAPI 对无信号会话同样不绑定；基座 forwarder 也有"生成 UUID 不能作为上游缓存 key"的既有注释。
- `RoutingSessionStore` 只保存 `RoutingSessionState`，容量默认 2000 个会话，LRU 淘汰；代理重启时状态清空，属于安全降级。
- 状态更新在一次内存锁操作中完成，避免并发请求对同一会话发生读改写丢失。回调不允许做数据库、网络或异步操作。
- 当前 Shadow 观察只接入 Claude 链路（`handle_messages_for_app`）；Codex/GrokBuild 的 session 提取已就绪但尚未接入 Shadow，属有意范围。

## 缓存保护策略（v0.4）

缓存保护的证据按可靠性分两级：

1. **真实反馈（主证据）**：Usage Finalize 阶段把上一轮真实响应的 `cache_read_tokens` 经 `update_existing_with` 写回会话状态（`last_cache_read_tokens` / `last_cache_read_at`）。Shadow 推荐从不执行，连续性必须锚在真实出站的命中证据上，而不是客户端在请求体里的声明。解析失败的零值 usage 不冲掉已有正反馈；key 不存在（匿名/已淘汰）时静默跳过。
2. **请求声明（辅助信号）**：本轮请求体含 `cache_control` 块（`cache_write_tokens > 0`），覆盖会话首轮等尚无反馈的场景。

保护生效还需同时满足：

- **TTL 净化**：`last_recommended_slot` 与真实缓存证据只在 `SLOT_PROTECTION_TTL_MS`（90 分钟，对齐 prompt cache 5m/1h 生命周期加余量）内有效。净化在 observer 层完成，决策引擎保持无时钟纯函数（PRD §12.3 的可重放约束）。
- **防锁存上限**：`consecutive_cache_protections >= MAX_CONSECUTIVE_CACHE_PROTECTIONS`（5）时强制按本轮阈值重新评估。否则一次瞬时高槽位推荐可以借"保护上一轮推荐"在活跃会话内无限自我维持，把保护机制的惯性污染成分类器判断。
- 上一轮槽位严格高于本轮阈值推荐；复杂度上升可升档，客户端显式小模型优先。

`safe_to_execute` 始终为 `false`；该策略只改变 `recommended_slot` 和解释，不改变请求 body、Provider Router、Forwarder 或真实出站模型/Provider。

## 已知取舍

- 会话读取-计算-提交在一把全局写锁内完成（`update_with`），包含特征提取。单机代理的并发量下这是简单且正确的选择；若未来成为瓶颈，按 key 分片即可，语义不变。
- failover 场景下 Usage Finalize 的实际 Provider 与 key 中的初始 Provider 不一致，该轮反馈静默丢弃——failover 后缓存连续性本就应重置。

## 未从上游吸收的部分

Stack mode 的真实路由、`prompt_cache_key` 注入、auth 绑定/重绑定、opaque state rectifier 的反应式重试、熔断退避、Merkle LCP 会话匹配，全部依赖真实出站改写或凭证级路由，越过 v0.1 Shadow 边界，留待 Live Canary 阶段评估。

## 版本与回放

策略版本 `shadow-policy-v0.4`：会话 key 加入 Provider、匿名会话不再建状态、缓存保护改以真实反馈为主证据、新增 TTL 净化与防锁存上限。历史决策仍保留各自的版本戳；导入/回放遇到策略版本不匹配时应继续按现有安全规则拒绝，而不是静默用新规则重算。

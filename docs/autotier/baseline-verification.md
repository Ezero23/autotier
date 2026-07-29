# AutoTier Phase 0 — 基线验证报告

> 验证对象：
> - `farion1231/cc-switch` @ `30409878bdbdf1c7091c559d6afc367a052da39c`(Package `3.18.0`;describe `v3.18.0-36-g30409878`)— **选定基座**
> - `BigStrongSun/ccswitchmulti` @ `2bbe8d204b8b57eb638132c02b8f715a70f530d5`（v3.16.5-22）— 对照基座
>
> 验证日期：2026-07-29。原则：只运行基座自带的构建/检查/测试，如实记录；不修改任何基座源码。

## 1. 验证环境

| 项 | 值 |
|---|---|
| 机器 | macOS（Apple Silicon, aarch64-apple-darwin） |
| Node | v24.18.0 |
| pnpm | 11.8.0（需 `COREPACK_ENABLE_PROJECT_SPEC=0`：用户主目录 `~/package.json` 声明了 `packageManager: yarn@4.x`，corepack 会在子目录拒绝 pnpm） |
| Rust | 1.95.0（两基座 `rust-toolchain.toml` 均锁定 channel 1.95 + rustfmt + clippy；首次安装因网络 TLS 中断失败，重装后成功） |
| pnpm 构建脚本 | 基座依赖 esbuild/msw 的 postinstall 被 pnpm 11 默认忽略导致 install 报错，需 `PNPM_CONFIG_STRICT_DEP_BUILDS=false`（仅放宽构建脚本审批，不影响检查结果） |

**Spike 期间对基座仓库的全部改动**：仅在每个仓库根目录 `mkdir -p dist`（CI 第 94-96 行的同款占位目录，untracked，供 tauri 构建宏使用）。未修改、未提交任何源码文件。

## 2. farion1231/cc-switch（选定基座）实测结果

| 检查 | 命令 | 结果 | 耗时 |
|---|---|---|---|
| 依赖安装 | `pnpm install` | ✅ 通过（首次因网络超时失败，重试成功） | ~1min |
| 前端类型检查 | `pnpm typecheck`（`tsc --noEmit`） | ✅ 通过，exit 0 | 秒级 |
| 前端 Lint（格式） | `pnpm format:check`（prettier；仓库无 ESLint 配置） | ✅ 通过，"All matched files use Prettier code style!" | 秒级 |
| 前端单元测试 | `pnpm test:unit`（vitest run） | ✅ **83 文件 / 567 用例全部通过** | 20.48s |
| Rust 格式 | `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | ✅ 通过，exit 0 | 秒级 |
| Rust Lint | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | ✅ 通过，exit 0（含依赖全量编译） | 4m48s |
| Rust 测试 | `cargo test --manifest-path src-tauri/Cargo.toml` | ✅ **全部通过：2,375 passed / 0 failed / 2 ignored**（lib 2,259 + 集成 116，13 个测试二进制全部 ok） | ~6min（含编译） |
| 测试规模 | grep 统计 | src 内 2,302 个 `#[test]`/`#[tokio::test]`；`src-tauri/tests/` 12 个集成测试文件 / 117 个用例 | — |
| 前端生产构建 | `pnpm run build:renderer`（vite build） | ✅ 通过（7.62s；仅基座自身的 chunk >500kB 警告） | 7.62s |
| Rust Debug Build | `cargo build --manifest-path src-tauri/Cargo.toml` | ✅ 通过（dev profile） | 1m00s |

CI 配置（`.github/workflows/ci.yml`）：前端 typecheck/format:check/test:unit；后端 fmt/clippy/test，矩阵 ubuntu-22.04、windows-latest、macos-latest。三端构建有 CI 覆盖；本次 Spike 仅在本机 macOS 实测。

## 3. BigStrongSun/ccswitchmulti（对照基座）实测结果

| 检查 | 命令 | 结果 | 耗时 |
|---|---|---|---|
| 依赖安装 | `pnpm install` | ✅ 通过 | 8.5s |
| 前端类型检查 | `pnpm typecheck` | ✅ 通过，exit 0 | 秒级 |
| 前端 Lint（格式） | `pnpm format:check` | ❌ **失败**：`src/lib/codexMultiRouterWizard.ts` 不符合 Prettier 风格（基座自身遗留，非 Spike 引入） | 秒级 |
| 前端单元测试 | `pnpm test:unit` | ✅ **82 文件 / 585 用例全部通过** | 17.89s |
| Rust 格式 | `cargo fmt --check` | ✅ 通过，exit 0 | 秒级 |
| Rust Lint | `cargo clippy -- -D warnings` | ❌ **失败**：`src/codex_history_migration.rs:3032` 触发 `clippy::manual_repeat_n`（`repeat().take()` 应改写为 `repeat_n()`），`-D warnings` 下编译中止。基座自身遗留，非 Spike 引入 | ~4min |
| Rust 测试 | `cargo test`（lib） | ❌ **1 失败 / 2,119 通过 / 2 ignored**：`proxy::providers::transform_codex_chat::tests::responses_request_does_not_emit_chat_file_for_url_only_input_file` 断言失败（Codex 转换层，基座自身遗留）。默认 fail-fast 中止后续 target，集成测试结果以 `--no-fail-fast` 重跑为准（见第 5 节） | ~2min（增量编译后） |
| 测试规模 | grep 统计 | src 内 2,165 个 `#[test]`；`src-tauri/tests/` 11 个集成测试文件 | — |

## 4. 关闭 AutoTier 后的 Parity 验证方法（设计，待后续 Phase 执行）

目标（PRD 0.2、NFR-003）：AutoTier 路由功能关闭时，代理行为与基座完全一致。Phase 0 确认可行的验证组合：

1. **代码路径级**：Shadow/Off 开关实现为 `handle_messages_for_app`（`handlers.rs:177-194`）插入点处的单次短路判断；Off 时不进行特征提取、不写决策、不触碰 body/headers。评审时以 diff 证明转发链路零改动。
2. **字节级抓包对比（核心证据）**：
   - 准备一组固定 Claude Code 请求样本（流式 + 非流式 + tool_use + 错误响应 + failover 场景）。
   - 在同一 mock 上游前分别运行：基座原始二进制 vs AutoTier Off 模式二进制，录制（a）发往上游的请求字节（method/URL/headers/body）、（b）返回客户端的响应字节（含 SSE 事件序列与时序容差）。
   - 断言两者逐字节一致（动态字段如 `x-api-key` 注入、时间戳做字段级白名单比对）。
3. **基座全量回归**：`cargo test`（2,302+117 用例）与 `pnpm test:unit`（567 用例）在 AutoTier 代码合入后必须全部通过，不得修改任何基座测试。
4. **DB 内容对比**：同一流量回放后，`proxy_request_logs` 内容（request_id、tokens、cost、session_id、status）与基座运行结果一致；Off 模式下 `autotier_*` 表零新增行。
5. **配置还原**：takeover 关闭路径（`src-tauri/src/services/proxy.rs:925`）还原 `~/.claude/settings.json` 后与基座一致（该路径基座已有测试，AutoTier 不触碰）。

## 5. Phase 0 Closure 运行冒烟证据（AMEND-001）

验证环境：`Ezero23/autotier` 仓库（merge commit `445e93c7`，导入基座与 `bases/cc-switch` 同 SHA，tree diff 仅文档类文件）。验证工具：`src-tauri/tests/proxy_smoke.rs`（仓库引导期唯一新增代码文件，约 640 行；内置 3 个 axum mock 上游：good / backup / 恒 500 bad）。

### 6.1 构建

| 检查 | 命令 | 结果 |
|---|---|---|
| 前端生产构建 | `pnpm run build:renderer` | ✅ 4.68s（仅基座自身 chunk 警告） |
| 前端类型检查 | `pnpm typecheck` | ✅ |
| Rust Debug Build | `cargo build --manifest-path src-tauri/Cargo.toml` | ✅ 1m01s |

注意：本机 pnpm 11 需要 `COREPACK_ENABLE_PROJECT_SPEC=0 PNPM_CONFIG_STRICT_DEP_BUILDS=false`，否则 install 会改写 `pnpm-workspace.yaml`（`allowBuilds` 占位）并在后续命令的 deps 检查中报错。

### 6.2 代理启动与五场景冒烟（`cargo test --test proxy_smoke`，串行）

代理经应用同一入口 `AppState.proxy_service.start()`（`services/proxy.rs:518`）启动，OS 随机端口；mock 上游实现 Anthropic `/v1/messages`。**两次运行（bases checkout 与正式仓库）全部 PASS，测试本体 0.06s。**

| 场景 | 结果 | 出站 model / provider / message_id |
|---|---|---|
| a 非流式 | ✅ 200，含 content + usage | `claude-sonnet-4-6` / `p1` / `msg_good_0000` |
| b 流式 | ✅ 200,SSE 序列 `message_start → content_block_start → delta → stop → message_delta → message_stop` | 同上 / `p1` / `msg_good_0001` |
| c 含 tools | ✅ 200;mock 确认收到 `tools` 字段,`tool_use`(get_weather）正常透传 | 同上 / `p1` / `msg_good_0002` |
| d 上游 500(failover 关） | ✅ 500 原样透传（状态码 + 上游 JSON 错误体）；failover 未触发 | `claude-force-500` / `p1` / 无 message id |
| e Failover(pfail 500 → pbackup) | ✅ 客户端 200;bad 被尝试 1 次后切到 backup,usage 归因 `pbackup` | `claude-sonnet-4-6` / **`pbackup`** / `msg_backup_0000` |

Failover 实测结论（非假设）：基座默认 `auto_failover_enabled=false` 时仅试当前 Provider 一次（max_retries=0),500 原样透传并记 status=500 日志；开启后 500 属 Retryable 分桶（`forwarder.rs:2678`，仅 400/405/406/413/414/415/422/501 为 NonRetryable)，按 failover 队列 `ORDER BY COALESCE(sort_index,999999), id ASC`（`dao/failover.rs:31`）顺序切换。

### 6.3 Usage Finalize 证据（`proxy_request_logs` 实测 dump)

```text
request_id=session:msg_good_0000   provider=p1      model=claude-sonnet-4-6  session=smoke-a  in=15 out=8  cost=0.000165 status=200 streaming=0
request_id=session:msg_good_0001   provider=p1      model=claude-sonnet-4-6  session=smoke-b  in=15 out=9  cost=0.000180 status=200 streaming=1 first_token_ms=0
request_id=session:msg_good_0002   provider=p1      model=claude-sonnet-4-6  session=smoke-c  in=15 out=8  cost=0.000165 status=200 streaming=0
request_id=9e810bbf-19aa-4390-...  provider=p1      model=claude-force-500   session=smoke-d  in=0  out=0  cost=0       status=500 error=上游错误 (500)
request_id=session:msg_backup_0000 provider=pbackup model=claude-sonnet-4-6  session=smoke-e  in=15 out=8  cost=0.000165 status=200 streaming=0
```

确认了 AMEND-001 的关键事实：成功请求 `request_id = session:{message_id}`（响应后才存在）；上游 500 无 message id 时退化为 UUID；failover 后 usage 归因到实际执行 Provider（`pbackup`），即 `actual_outbound_provider != initial_selected_provider` 在基座上真实存在——新不变量（actual == baseline，而非 actual == client_requested）是必要的。

## 6. 结果更新记录

- 2026-07-29：前端检查全部完成并记录；`cargo fmt --check` 两基座通过；cc-switch `cargo clippy` 通过（4m48s）。
- 2026-07-29（最终）：
  - **cc-switch `cargo test` 全部通过**：2,375 passed / 0 failed / 2 ignored（lib 2,259 + 12 个集成测试二进制共 116）。
  - **ccswitchmulti `cargo clippy -D warnings` 失败**：`codex_history_migration.rs:3032` `manual_repeat_n`，基座自身遗留。
  - **ccswitchmulti `cargo test --no-fail-fast` 全量**：lib 2,119 passed / **1 failed**（上述 Codex 用例）/ 2 ignored；10 个集成测试二进制共 **98 passed / 0 failed**。即唯一失败为基座自身的 1 个 Codex 转换层用例。
- 2026-07-29（Closure）：正式仓库（merge `445e93c7`）内完成 renderer build / typecheck / cargo build / 五场景 proxy 冒烟，全部通过，证据见第 5 节。

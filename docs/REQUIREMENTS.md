# ModelHub — 产品需求与技术规格

> 状态：**现行实现规格**（随开发迭代更新，非早期冻结草稿）  
> 最后更新：2026-07-24（晚间：Providers/Import/主框架修订）  
> 项目代号：**ModelHub**  
> 仓库：https://github.com/protosse/ModelHub  

本文档描述 **当前产品意图 + 已实现行为 + 明确不做的范围**。若代码与文档冲突，以代码为准并回写本文档。

---

## 1. 产品定位

ModelHub 是跨平台桌面应用，用于 **以模型提供商（Provider）为入口** 统一管理 AI coding agent 的模型相关配置，并按 Agent 差异写出到本机配置文件。

### 1.1 支持的 Agent

| Agent | 说明 | 主要配置路径（默认） |
|-------|------|----------------------|
| **Claude Code** | Anthropic CLI | `~/.claude/settings.json` |
| **Codex** | OpenAI Codex CLI | `~/.codex/config.toml`（Apply **不修改** `auth.json`） |
| **OpenCode** | 开源 coding agent | `~/.config/opencode/opencode.json`（或 `.jsonc`）；密钥：`options.apiKey` 或 `~/.local/share/opencode/auth.json`；最近主模型：`~/.local/state/opencode/model.json` |
| **Pi** | pi coding agent | `~/.pi/agent/models.json` + `~/.pi/agent/settings.json` |

路径可在 `~/.modelhub/config.json` 的 `paths` 中覆盖。

### 1.2 与其它工具的关系（概念层）

| 维度 | 典型「按 Agent 管理」工具 | ModelHub |
|------|---------------------------|----------|
| 入口 | 先选 Agent，再配 Provider | **先配 Provider/Model**，再分配给 Agent |
| 痛点 | 同一中转站在每个 Agent 下重复配置 | Provider（及 key）配一次，按需分配 |
| 范围 | 常含 MCP / Skills / 代理 / 用量等 | **只做模型配置** + **连通性测试** |
| 状态 | 常有自己的「当前启用」库 | **只改各 Agent 原生配置文件**；不维护第三方「当前启用」状态 |

> 界面文案不依赖其它具体产品名。用户若同时使用其它配置切换工具，两边 UI 状态可能不一致，**以各 Agent 磁盘配置文件为准**。

### 1.3 非目标（当前版本明确不做）

- 本地代理 / 请求劫持 / 协议转换网关 / Failover / 用量统计
- **提供商余额 / 额度查询与展示**
- MCP / Skills / Prompts / Sessions 统一管理
- 云同步
- OS Keychain 加密密钥（当前：明文本地文件 + 建议 `0600`）
- Electron；SQLite（`~/.modelhub/*.json`）
- 自动同步第三方桌面工具的「当前启用」库
- OpenCode / Pi **分 Agent 独立同步提供商列表**（当前共用全局 `Provider.enabled`；后续可选）
- 导入时按模型粒度勾选

---

## 2. 核心心智模型

```text
ModelHub 库（持久）
  Provider（名称唯一）
    └─ Models[]
  testPrompts / modelTestResults（测试相关）

会话草稿 draftBindings（仅内存）
  各 Agent 的 Active / 官方|第三方
       │
       ▼  应用同步 Apply（可 Diff 预览）
  写出到各 Agent 原生配置
```

### 2.1 关键原则

1. **Provider-first**：库内真相源是 Provider + Model。  
2. **不同 API Key ≈ 不同 Provider 实例**；支持克隆换 key。  
3. **Provider 名称全局唯一**（大小写不敏感）。  
4. **Agent 写出分两类**：  
   - Claude / Codex：只设 **当前默认（Active）**  
   - OpenCode / Pi：**enabled 的 Provider 全量目录** + Active 主模型（两端共用 `enabled`）  
5. **Agent 绑定草稿不写 store**：修改即时进会话内存；首次/重置从磁盘读；切换 Tab 不丢（页面 keep-alive）。  
6. **Apply 可预览 Diff**；密钥仅当真变化才标变更。  
7. **连通性测试**：真实 API 调用；详细日志仅内存；摘要可落盘。

---

## 3. 功能需求（按模块）

### 3.1 提供商（Providers）

#### 3.1.1 字段

| 字段 | 说明 |
|------|------|
| name | 展示名，**全局唯一** |
| baseUrl | API 根（规范化 trim、去尾 `/`） |
| protocol | `openai-completions` \| `openai-responses` \| `anthropic-messages` |
| apiKey | 明文 secrets；UI 遮罩/显示/复制 |
| headers / compat | 可选 |
| enabled | 是否参与 OpenCode / Pi 全量写出 |
| notes | 备注 |

#### 3.1.2 操作

- 新建 / 编辑 / 删除（二次确认）；列表多选、全选、批量删除  
- **列表搜索**：**仅匹配 Provider 名称**（大小写不敏感子串）；不搜 URL / 协议  
- **全选**：仅作用于**当前搜索结果（可见列表）**；勾选=并入可见 ID，取消=仅去掉可见 ID；筛选外已勾项保留（与导入页「清空当前」同语义）  
- **空态**：库为空 →「暂无提供商…」；库非空但搜索无命中 →「无匹配结果」  
- 删除提供商后：清勾选 / 远程模型缓存 / 详情选中；会话 `draftBindings` 中悬空 Provider/Model 引用在 `get_state` 刷新时 **scrub 掉**（避免 Apply 报「Provider 不存在」）  
- **克隆**：复制 URL/协议/headers/模型，换名称与 key；失败时弹窗内错误、保持打开  
- 启用/禁用同步（仅影响 OC/Pi 全量写出）  
- Toast：右下角浮层；连续 Toast **重置计时**（不互相提前关掉）；**启用/禁用成功、测试完成/停止不弹成功 Toast**；失败与阻断必须提示  
- **主导航 Tab 保活**（visit-then-keep-alive）：切走再回来保留页面本地状态；Providers 页隐藏时 **暂停** test-session 订阅，回到页时再订阅并刷新展示  

#### 3.1.3 模型

- 详情 **默认打开「模型」Tab**（模型在前、连接在后）  
- 手动添加；可从远程列表多选添加（**后端 `add_models` 批量一次落盘**）  
- **从已获取列表选择**：进入时**不预勾**第一项；支持 **搜索 Model ID / 名称**；提供 **全选**（仅当前搜索结果，筛选外已勾保留）与「已选 N / M」；未勾选不可提交  
- **获取模型**：`{baseUrl}/models` 或 `{baseUrl}/v1/models`（async，「获取中…」，防重复点击）  
- 远程列表缓存按 **providerId** 隔离；**编辑 baseUrl/协议等连接信息后清空该 Provider 缓存**；删除 Provider 时同步清缓存  
- 编辑 Model ID / 展示名 / 启用；Model ID 可手输，有远程缓存时可选取回填  
- 启用开关样式明显（已启用 / 已禁用）  
- **baseUrl 可点击**：系统浏览器打开（热区限文字）  

##### 连通性测试

入口（均在模型侧）：

1. 模型行 **「测试」**  
2. 详情 **「测试全部」**  
3. 列表 **「测试所选」**（先勾选提供商）  

| 能力 | 行为 |
|------|------|
| 单模型 | 提示词、超时 5–300s（默认 30）、真实请求；**网络请求日志默认收起**（开始测试 / 回填历史日志均不自动展开，仅用户点「展开」）；**停止**：立即结束 UI 等待并丢弃本轮结果（已发出的 HTTP 无法在客户端强杀，可能仍跑完）；event `test-connection-log` |
| 关弹窗 | **不中断**；single / batch / multi 模块级 session 保活 |
| 测试全部 | **串行**；可选仅已启用；**停止**：待测立刻标跳过，当前请求跑完后结束会话；行内响应时间：优先本轮 `result.latencyMs`，否则回退 `modelTestResults.latencyMs`（重启后仍显示） |
| 测试所选 | **全局并发 3**，**同提供商串行**；**停止**：待测立刻标跳过，进行中请求跑完后结束会话；响应时间回退规则同「测试全部」 |
| 额外请求头 | 三个测试弹窗均可编辑（`Key: Value` 多行）；合并顺序 **provider.headers → 本轮 extraHeaders**（同名覆盖）；默认：`anthropic-messages` → `User-Agent: claude-cli/2.1.79` + `x-app: cli`；OpenAI 系 → `User-Agent: openai-node`；「填默认」可恢复；仅本轮测试生效，不写回 Provider |
| 跨入口共享 | `lastTestResults` + session；列表测过的结果可在详情看到 |
| 提示词 | `store.testPrompts`；默认种子「连通性探测」内容：`将123@qq.com转为Base64，直接回复结果`；可设默认/删（默认不可删）；**单测与批量弹窗均支持保存/设默认/删除** |
| 日志 | 脱敏；详细日志 **仅内存**；清空单测日志同步清共享缓存，避免批量结果回填 |
| 最近测试列 | 成功/失败/测试中/待测/跳过 + **响应时间（ms）**（有则紧挨徽章显示）；摘要 `modelTestResults` 落盘含 `ok` / `testedAt` / `latencyMs`，重启后 hydrate 恢复状态与耗时；完整日志仅内存 |
| 协议 | completions / responses / anthropic-messages；completions/anthropic 用小 `max_tokens`；**responses 不发送 `max_output_tokens`**（兼容拒绝该参数的第三方网关） |
| 触发 | **仅用户点击发送/开始** 才请求 |

### 3.2 导入（Import）

#### 3.2.1 数据源与取 Key

| 源 | 配置 | Key 来源 |
|----|------|----------|
| OpenCode | `provider.*` | `options.apiKey` **优先**，其次 auth.json |
| Pi | `models.json` | `apiKey` |
| Claude | `settings.json` env | `ANTHROPIC_AUTH_TOKEN` / `API_KEY` |
| Codex | `[model_providers.*]` | ① `experimental_bearer_token` ② `requires_openai_auth` 时 `~/.codex/auth.json` 的 `OPENAI_API_KEY` ③ 兜底 auth.json 有 key 则用 |

扫描失败时 `scanNotes` 按源给出可读错误（解析失败等），不阻断其它源。

#### 3.2.2 合并与去重

- **同一 baseUrl + protocol** = 同一端点，跨 Agent **合并一行**（来源 `opencode+pi+codex`）  
- 合并时 **优先保留非空 apiKey**  
- **已存在**：同端点已在 ModelHub（**不比较模型集合**）  
- **名称**：全局唯一；冲突可改名 / 自动改名（`name-2` 递增避让）  

#### 3.2.3 覆盖语义（增量）

勾选已存在项 = **覆盖**：

1. 更新 name / baseUrl / protocol；notes 仅在原为空时补写来源  
2. **保留** 用户已有 `headers` / `compat` / `enabled`（不被导入覆盖）  
3. **仅当扫描到非空 Key 时** 更新密钥（空 Key **不覆盖** 已有 secrets）  
4. **模型增量**：只添加库中尚未有的 model id；**不删除** 已有模型  

预览字段：`extraModelCount`、`newModelIds`（绿）、`existingModelIds`（灰）。

#### 3.2.4 默认勾选与动作

| 行类型 | 默认勾选 | 勾选后动作 |
|--------|----------|------------|
| 新 + 有 Key | ✓ | 导入（新建） |
| 新 + 无 Key | ✗ | 若勾选 → 导入（结果提示补 Key） |
| 已存在 | ✗ | 若勾选 → **覆盖（增量）** |
| 不勾选 | — | 跳过 |

无「动作」下拉：**不勾选 = 跳过**。

#### 3.2.5 筛选与批量勾选（精简）

**状态筛选（互斥 chip）：**

| Chip | 含义 |
|------|------|
| 全部 | 全部扫描项 |
| **可导入** | 新提供商（`!alreadyExists`） |
| **可补模型** | 已存在且 `extraModelCount > 0` |
| **已存在** | `alreadyExists` |

另有：**来源**（全部 / OpenCode / Pi / Claude / Codex）+ **搜索**名称/URL。

**批量勾选（始终针对当前筛选列表，可叠加）：**

| 按钮 | 行为 |
|------|------|
| 勾选可导入 | 当前列表中「新 + 有 Key」 |
| 勾选可补模型 | 当前列表中「已存在且有可补」 |
| 勾选已存在 | 当前列表中全部已存在 |
| 清空当前 | 仅取消当前列表勾选（筛选外保留） |

计数：`已选 N`；有筛选时显示当前列表项数/已选，及「含筛选外 X 项」。

列表排序：新+有 Key → 新无 Key → 可补模型 → 其它；同组按名称。

#### 3.2.6 校验与确认

- 名称空 / 本批重名 / 与库冲突（**含覆盖行改名撞到其它 Provider**）：行内红字；失败时 **清筛选并滚到首个错误行**  
- 实时冲突检测与提交校验共用同一套规则；**仅勾选中的行**参与冲突（未勾选的已存在行不误报）  
- 自动改名：在当前名称根上 `-2` 递增，**不剥**名称末尾已有的 `-N`（避免 `gpt-4` → `gpt-2`）  
- 导入前 **ConfirmDialog**：新建数 / 覆盖数 / 预计 +模型数 / 无 Key 数  
- 导入中 `importing` 与扫描 `scanning` 分离  
- 导入失败：可能已部分落盘 → 刷新库 + **保留勾选/改名** 再扫（`keep` 模式），避免 UI 与磁盘不一致  

#### 3.2.7 导入后

- 单条 Toast 摘要（含无 Key 提示）  
- 结果条：无 Key 列表（导入时 **id+名称快照**，二次扫描丢项仍可显示）+ **查看提供商**  
- 刷新预览（`clear` 模式：保留改名、全部取消勾选）  

#### 3.2.8 状态模型与保活

- 前端单源：`ImportItem = ImportPreviewItem + selected + error`（无 preview/rows 双表）  
- 重扫合并模式：`defaults`（默认勾选）/ `keep`（保留勾选与改名）/ `clear`（保留改名、全不勾）  
- **Tab 保活**：再次进入时若库（providers/models）相对上次扫描已变，**静默 `keep` 重扫**，避免陈旧「已存在/可补」  

#### 3.2.9 行 UI

- 卡片行：名称可编辑、状态 badge、协议缩写、模型展开  
- 展开模型列表：**绿色** = 将新增，**灰色** = 库内已有（跳过）；无图例文案  
- 无 Key 行略透明  
- 行空白点击切换勾选  
- 名称冲突显示「自动改名」（实时 warning + 提交后行内红字）；用户改名/勾选变化后 **按当前状态重算**：已修好的行消红，仍无效的行保持标红（不会一次清掉全部）  

组件：`ImportPage.tsx` + `ImportRow.tsx`。

### 3.3 Agent 绑定（会话草稿）

#### 3.3.1 生命周期

| 事件 | 行为 |
|------|------|
| 本会话首次进入（无草稿） | `read_live_bindings` 读磁盘并匹配库内 Provider/Model |
| 修改 | **即时**写入 `draftBindings`（无保存按钮） |
| 切换 Tab | 不重新读盘（keep-alive） |
| 重置 | 再读磁盘覆盖草稿 |
| 库变更刷新（删除 Provider/Model 等） | `scrubBindings`：去掉草稿中已不存在的 providerId/modelId |
| 关闭应用 | 草稿丢失 |
| 应用同步 | 请求携带 `bindings` |

#### 3.3.2 各 Agent

| Agent | 绑定 | 磁盘解读要点 |
|-------|------|----------------|
| Claude | 官方/第三方；Provider+Model | BASE_URL 空 → 官方 |
| Codex | 同上；记录 `providerKey` | `model_provider` 为 openai/空 → 官方 |
| OpenCode | 默认 Provider+Model（+small） | config `model` 优先，否则 `model.json` recent[0] |
| Pi | 默认 Provider+Model | settings `defaultProvider` / `defaultModel` |

切换 Provider 时 **自动选中该 Provider 下第一个模型**。

### 3.4 应用同步（Apply）

1. 选择 Agent（默认全选；可全选/清空）  
2. Diff：磁盘 → 按会话草稿 Apply 后  
3. 确认 → 备份 → 写出 → 结果/重启提示  

#### Diff

- `=` / `+` / `-` / `~`  
- 密钥比较真实 token；相同则 unchanged  
- 写出 Provider key 优先复用磁盘同 baseUrl 的 map key  

#### 写出规格摘要

| Agent | 要点 |
|-------|------|
| Claude | env + model；**不写** `_modelhub` |
| Codex | `modelhub` 槽 + `experimental_bearer_token`；**不改** auth.json |
| OpenCode | 合并 enabled providers + 默认 model |
| Pi | 合并 enabled + defaultProvider/Model |

### 3.5 备份

- Apply 前备份到 `~/.modelhub/backups/<agent>/<timestamp>/`  
- `backupKeepCount` 默认 10  

### 3.6 设置

- 语言、备份份数、数据目录、路径检测（路径编辑可后续增强）  

---

## 4. 协议与兼容

| protocol | 主要消费者 |
|----------|------------|
| `anthropic-messages` | Claude；部分 OC/Pi |
| `openai-completions` | OpenCode、Pi、多数中转 |
| `openai-responses` | Codex（强相关） |

分配给 Codex 且非 responses 时 UI 警告。

---

## 5. 本地存储

```text
~/.modelhub/
  config.json
  store.json     # providers, models, testPrompts, modelTestResults
  secrets.json   # 0600
  backups/
```

| 数据 | 持久化 |
|------|--------|
| Provider / Model / secrets | 是 |
| testPrompts / modelTestResults | 是（测试摘要，无完整日志） |
| 连通性详细日志 / test session | **否**（内存） |
| agentBindings 草稿 | **否**（内存 + Apply 请求） |

用户密钥与 Agent 配置在 **家目录**，不在应用仓库内。

---

## 6. Store 与 API 摘要

### 6.1 类型

- `Provider` / `Model` / `Secrets`  
- `TestPrompt` / `ModelTestResult` / `TestConnectionRequest|Result`  
- `AgentBindings`  
- `ApplyRequest`：`agents` + 可选 `bindings`  
- `ImportPreview`：`items` + `scanNotes`  
- `ImportPreviewItem`：含 `modelIds` / `extraModelCount` / `newModelIds` / `existingModelIds` / `hasApiKey` 等  
- `ImportRequest`：`items: { id, name, action }[]`  

### 6.2 主要命令

| 命令 | 用途 |
|------|------|
| get_state | 库 + 路径 + 密钥遮罩 |
| Provider/Model CRUD、clone、enabled、delete 批量 | 库维护 |
| add_models | 批量添加模型（单次 load+save） |
| fetch_provider_models | 远程模型列表 |
| preview_import / run_import | 导入 |
| read_live_bindings | 磁盘绑定 |
| preview_apply / apply_config | Diff 与写出 |
| list_backups / reveal_api_key | 备份与密钥 |
| test_model_connection | 连通性测试 + 日志 event |
| list/upsert/delete_test_prompt / set_default_test_prompt | 提示词 |
| record_model_test_result | 最近测试摘要 |

---

## 7. 页面信息架构

```text
ModelHub
├── 提供商      # 列表 + 详情（默认模型 Tab）；测试全部 / 测试所选
├── Agent 绑定  # 磁盘 + 即时草稿 + 重置
├── 应用同步    # 选 Agent + Diff + Apply
├── 导入        # 扫描 / 筛选 / 勾选 / 确认
├── 备份
└── 设置
```

- 全局：「应用更改」→ 应用同步  
- 弹窗：Esc / 遮罩关闭；删除二次确认  
- Toast：右下角  

### 建议源码结构

```text
src/
  pages/          Providers Agents Apply Import Backups Settings
  components/     Layout Modal Toast ImportRow TestConnection*
  lib/            *TestSession lastTestResults testDisplay openExternal
src-tauri/src/
  store/ adapters/  # claude codex opencode pi import live preview
                    # fetch_models test_connection
  commands.rs paths.rs backup.rs
```

---

## 8. 技术栈

| 层 | 选型 |
|----|------|
| 壳 | Tauri 2 |
| 前端 | React + TypeScript + Tailwind |
| 后端 | Rust（serde_json / toml / reqwest async） |
| 包管理 | pnpm |
| 数据 | `~/.modelhub` JSON |

---

## 9. 决策清单（现行）

| # | 决策 |
|---|------|
| 1 | Tauri + React；跨 macOS / Windows / Linux |
| 2 | Provider-first；名称唯一；不同 key 可多实例 |
| 3 | Claude/Codex 只 Active；OpenCode/Pi 全量 enabled + Active（共用 enabled） |
| 4 | 密钥明文 secrets（0600）；UI 遮罩/显示/复制 |
| 5 | 无本地代理；重启提示代替热切换 |
| 6 | 只做模型配置 + 连通性测试 |
| 7 | 导入：端点合并；覆盖=增量模型；空 Key 不覆盖 secrets |
| 8 | OpenCode key：`options.apiKey` 优先于 auth.json |
| 9 | OpenCode 主模型：config 无则读 model.json recent |
| 10 | Codex Apply：`experimental_bearer_token`，不改 auth.json |
| 11 | Codex 导入：读 experimental_bearer_token 与 auth.json OPENAI_API_KEY |
| 12 | Agent 绑定：磁盘读 + 即时会话草稿，无保存按钮 |
| 13 | Apply 前 Diff；密钥仅当真变化才标 changed |
| 14 | 写出 Provider key 优先复用磁盘同 URL 的 key |
| 15 | 不向 Claude settings 写 `_modelhub` |
| 16 | 切换 Provider 自动选第一个模型 |
| 17 | 导入筛选精简：全部 / 可导入 / 可补模型 / 已存在 + 来源 + 搜索 |
| 18 | 导入勾选：可导入 / 可补模型 / 已存在 / 清空当前（均作用当前筛选） |
| 19 | 连通性：三入口、session 保活、日志内存、摘要落盘 |
| 20 | 默认测试提示词：Base64 转换句；三弹窗均可管理提示词 |
| 21 | 余额/额度不做 |
| 22 | 提供商列表全选/导入批量勾选：均只动**当前可见**项 |
| 23 | 提供商列表搜索：仅名称子串（不搜 URL/协议） |
| 24 | 删除库项后 scrub 会话 draftBindings |
| 25 | 导入覆盖保留 headers/compat/enabled；失败后 keep 重扫 |
| 26 | 添加模型：远程列表不预选、可全选；批量 `add_models` |

---

## 10. 用户主流程

```text
1. （可选）导入：刷新扫描 → 筛选 → 勾选 → 确认 → 补 Key
2. 提供商：维护模型、获取模型、连通性测试、启用同步
3. Agent 绑定：重置读盘 → 调整 Active（即时草稿）
4. 应用同步：看 Diff → 选 Agent → 写出 → 按需重启
```

---

## 11. 已知限制与后续

| 项 | 说明 |
|----|------|
| Codex 与历史 custom 并存 | 正常；运行时只看 `model_provider` |
| 与其它切换工具 UI | 不同步；以磁盘为准 |
| OC/Pi 同步目录 | 共用 `enabled`；分 Agent 列表为后续可选 |
| 会话草稿 / 测试详细日志 | 跨重启故意不持久化；草稿会随库删除 scrub |
| 导入 keep-alive 静默重扫 | 以 store 指纹为准；仅 Agent 配置文件变更需点「刷新扫描」 |
| 备份一键恢复、设置路径编辑 | 可增强 |
| Keychain | 可选后续 |
| 提供商余额 | 明确不做 |

---

## 12. 修订记录

| 日期 | 说明 |
|------|------|
| 2026-07-23 | 初版；重写对齐实现；连通性测试与多轮 UX |
| 2026-07-24 | 绑定即时草稿、无保存按钮；选 Provider 自动首模型 |
| 2026-07-24 | 导入 P0/P1/P2：筛选精简、覆盖增量、Codex auth key、模型绿/灰、可补勾选、拆 ImportRow、scanNotes |
| 2026-07-24 | **全文同步**：导入筛选/勾选语义、连通性三入口与提示词、决策清单与限制对齐现行代码 |
| 2026-07-24 | Providers：搜索协议 token、可见全选、空/无匹配文案、远程模型不预选+全选、批量 add_models、删项 scrub 草稿与缓存；Import：单源 items、覆盖保留 headers/compat/enabled、冲突校验统一、失败 keep 重扫、库变更静默 keep 重扫、无 Key 快照；Toast 计时重置；测试订阅 active 门控 |
| 2026-07-24 | 连通性测试：`openai-responses` 请求体去掉 `max_output_tokens`（兼容拒绝该参数的第三方网关）；completions/anthropic 仍用小 `max_tokens` |
| 2026-07-24 | 连通性测试：三弹窗支持额外请求头（默认 Claude Code / openai-node 客户端标识）；`test_model_connection.extraHeaders` 合并 provider.headers |
| 2026-07-24 | 最近测试列展示持久化的 `latencyMs`（成功/失败旁显示 ms；重启后从 `modelTestResults` 恢复） |


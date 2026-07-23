# ModelHub — 产品需求与技术规格

> 状态：**现行实现规格**（随开发迭代更新，非早期冻结草稿）  
> 最后更新：2026-07-23  
> 项目代号：**ModelHub**  
> 仓库：`/Users/edy/code/ModelHub`

本文档描述 **当前产品意图 + 已实现行为 + 明确不做的范围**。若代码与文档冲突，以代码为准并回写本文档。

---

## 1. 产品定位

ModelHub 是跨平台桌面应用，用于 **以模型提供商（Provider）为入口** 统一管理 AI coding agent 的模型相关配置，并按 Agent 差异写出到本机配置文件。

### 1.1 支持的 Agent

| Agent | 说明 | 主要配置路径（默认） |
|-------|------|----------------------|
| **Claude Code** | Anthropic CLI | `~/.claude/settings.json` |
| **Codex** | OpenAI Codex CLI | `~/.codex/config.toml`（**不修改** `auth.json`，除非未来另议） |
| **OpenCode** | 开源 coding agent | `~/.config/opencode/opencode.json`（或 `.jsonc`）；密钥还可在 `options.apiKey` / `~/.local/share/opencode/auth.json`；**最近主模型**在 `~/.local/state/opencode/model.json` |
| **Pi** | pi coding agent | `~/.pi/agent/models.json` + `~/.pi/agent/settings.json` |

路径可在应用配置 `~/.modelhub/config.json` 的 `paths` 中覆盖。

### 1.2 与其它工具的关系（概念层）

| 维度 | 典型「按 Agent 管理」工具 | ModelHub |
|------|---------------------------|----------|
| 入口 | 先选 Agent，再配 Provider | **先配 Provider/Model**，再分配给 Agent |
| 痛点 | 同一中转站在每个 Agent 下重复配置 | Provider（及 key）配一次，按需分配 |
| 范围 | 常含 MCP / Skills / 代理 / 用量等 | **只做模型配置** |
| 状态 | 常有自己的「当前启用」库 | **只改各 Agent 原生配置文件**；不维护第三方工具的「当前启用」状态 |

> 界面文案 **不** 依赖其它具体产品名；用户若同时使用其它配置切换工具，两边「当前启用」UI 可能不一致，以各 Agent 磁盘配置文件为准。

### 1.3 非目标（当前版本明确不做）

- 本地代理（Local Proxy）/ 请求劫持 / 协议转换网关 / Failover / 用量统计
- **提供商余额 / 额度 / 剩余额度查询与展示**（含 NewAPI `/api/usage/token/`、账号 `/api/user/self`、官方 balance 等；调研后明确本期不做）
- MCP / Skills / Prompts / Sessions 统一管理
- 云同步
- OS Keychain 加密密钥（当前：明文本地文件 + 建议 `0600`）
- Electron；持久化不用 SQLite（`~/.modelhub/*.json`）
- 自动同步第三方桌面工具的「当前启用」库

---

## 2. 核心心智模型

```text
ModelHub 库（持久）
  Provider（名称唯一；不同 key 建议不同实例）
    └─ Models[]（归属某 Provider；可启用/禁用）

会话草稿 draftBindings（仅内存，关应用丢失）
  各 Agent 的 Active 选择 / 官方|第三方模式
       │
       ▼  应用同步 Apply
  写出到各 Agent 原生配置文件（可先 Diff 预览）
```

### 2.1 关键原则

1. **Provider-first**：库内真相源是 Provider + Model 列表。  
2. **不同 API Key ≈ 不同 Provider 实例**（即使 baseUrl 相同）；支持「克隆」换 key。  
3. **Provider 名称全局唯一**（大小写不敏感）。  
4. **Agent 行为分两类**（见 §3.3）：  
   - Claude / Codex：只设 **当前默认**（Active）  
   - OpenCode / Pi：同步 **enabled 的 Provider 全量目录** + 设默认主模型  
5. **Agent 绑定草稿不写 store**：仅会话内存，供本次 Apply；**修改即时进草稿**，无需单独「保存」；展示首次从 **磁盘实时读取**，切换 Tab 不重新读盘。  
6. **Apply 可预览 Diff**：对比磁盘现状与将要写入的内容；密钥类仅当真变化才标变更，不展示明文。

---

## 3. 功能需求（按模块）

### 3.1 提供商（Providers）

#### 3.1.1 CRUD 与字段

| 字段 | 说明 |
|------|------|
| name | 展示名，**全局唯一** |
| baseUrl | API 根地址（规范化：trim、去尾 `/`） |
| protocol | `openai-completions` \| `openai-responses` \| `anthropic-messages` |
| apiKey | 明文存 secrets；UI 默认遮罩，可显示/复制 |
| headers / compat | 可选 |
| enabled | 是否参与 OpenCode / Pi 全量同步 |
| notes | 备注 |

#### 3.1.2 操作

- 新建 / 编辑 / 删除（二次确认弹窗）
- 列表多选、全选、批量删除（二次确认）
- **克隆**：复制 URL/协议/headers/模型列表，换名称与 key
- 启用/禁用同步（仅影响 OC/Pi 全量写出）
- Toast：全局右下角浮层，不占布局高度；**失败/阻断操作**必须提示；下列情况**不**弹成功 Toast（页面状态已反馈）：
  - 模型启用/禁用切换成功
  - 连通性测试完成/停止（单测、测试全部、测试所选）
- **Tab 保活**：主导航切换不卸载已访问页面（visit-then-keep-alive），详情内本地状态（选中 Provider、搜索、模型编辑等）在切走再回来后保留

#### 3.1.3 模型子模块

- 提供商详情 **默认打开「模型」Tab**（「模型」在前、「连接」在后）
- 手动添加；可批量从远程列表勾选添加  
- **获取模型**：请求 `{baseUrl}/models` 或 `{baseUrl}/v1/models`（async，按钮显示「获取中…」，防重复点击）  
- 远程列表缓存按 **providerId** 隔离，不跨 Provider 共享  
- 编辑 Model ID / 展示名 / 启用：
  - Model ID 始终可手输；若有远程缓存，输入框右侧按钮打开远程列表，点选后回填（不改为纯下拉）
  - 编辑行高度与列宽与只读行对齐（`table-fixed`），避免布局跳动  
- 启用开关样式需明显（已启用 / 已禁用）
- **baseUrl 可点击**：列表与详情中的 baseUrl 用系统默认浏览器打开（`plugin-opener`）；点击热区仅限文字宽度，避免误触

##### 连通性测试

入口（均在模型侧，非「连接」Tab）：

1. **模型行「测试」**（按钮文案恒为「测试」；进行中显示「测试中…」）
2. 详情模型 Tab **「测试全部」**
3. 提供商列表 **「测试所选」**（与「删除所选」并列；需先勾选提供商）

| 能力 | 行为 |
|------|------|
| 单模型测试 | 弹窗：选/编提示词、超时（5–300s，默认 30）、发送后真实请求；网络日志**默认收起**，发送后可展开；实时日志流（Tauri event `test-connection-log`） |
| 关闭弹窗 | **不中断**进行中的请求；单测 / 测试全部 / 测试所选均用**模块级 session** 保活，再次打开可续看进度与结果 |
| 批量「测试全部」 | **串行（并发 1）**；可选「仅测试已启用」；每模型独立日志卡（默认收起，点击展开；`whitespace-pre-wrap` 换行，禁止横向滚动）；可停止（当前请求结束后不再测后续）；模型列表可**排序**（默认 / 响应时间升序 / 降序，仅影响展示） |
| 列表「测试所选」 | 勾选后配置提示词/超时再发送；**全局并发 3**，**同一提供商内模型串行**（不同提供商可并行）；关弹窗不中断；模型列表可**按提供商筛选**（「全部」+ 各提供商 chip，默认全亮，点按切换高亮，列表仅显示高亮提供商的模型）与**排序**（默认 / 响应时间 ↑ / ↓，无延迟数据排后；仅影响展示，不改测试队列） |
| 勾选变更与进行中会话 | 若已有 multi 会话在跑，改勾选再打开弹窗时**仍展示进行中会话**（提示与当前勾选不一致）；**不**用新勾选顶替进行中的 session；须等结束或停止后再开新一轮 |
| 跨入口状态共享 | 单测 / 测试全部 / 测试所选共用内存中的**最近日志 + 结果**（`lastTestResults` + session 行）；列表测过的模型，在详情「测试全部」/单测弹窗中可见状态与日志；详情「最近测试」列亦反映 multi/batch 进行中状态 |
| 提示词 | 存 `store.json` → `testPrompts`；可保存/更新/删除；任意一条可「设为默认」；**默认提示词不可删除**（须先改默认）；保存后选中刚保存的提示词 |
| 密钥与日志 | 请求日志脱敏（不回显明文 key）；结果含 timeline / request / response 明细（截断）；详细日志**仅会话内存**，**不**写入 `store.json` |
| 「最近测试」列 | 模型表显示 **成功 / 失败 / 测试中 / 待测 / 跳过 / —**；进行中会话优先于磁盘上次结果（新一轮队列中的模型显示「待测」，不再卡在旧「成功」）；**结果摘要**（ok、testedAt、latencyMs）持久化到 `store.json` → `modelTestResults`（按 `Model.id`）；悬停显示测试时间与可选延迟；删除模型/提供商时清理对应记录 |
| 协议 | `openai-completions` → chat/completions；`openai-responses` → responses；`anthropic-messages` → messages；小 `max_tokens` |
| 触发时机 | **仅用户点击发送/开始测试时**才发网络请求（实现与联调中不得自动探活） |
| 冗余反馈 | 测试完成/停止**不**弹成功 Toast；启用/禁用成功**不**弹 Toast（列状态与徽章已反馈）；错误与阻断仍 Toast |
| 稳定性 | 详情「测试全部」在仅有列表 multi 会话、无 batch session 时须正常打开（不得因空 session 崩溃）；batch 与 multi 对同一提供商互斥启动 |

### 3.2 导入（Import）

#### 3.2.1 数据源

| 源 | 读取内容 |
|----|----------|
| OpenCode | `provider.*`；key：`options.apiKey` 优先，其次 auth.json |
| Pi | `models.json` providers + apiKey |
| Claude | `settings.json` 的 `env.ANTHROPIC_*` + model |
| Codex | `config.toml` 的 `[model_providers.*]` + 顶层 model |

#### 3.2.2 合并与去重

- **同一 baseUrl + protocol** 视为同一端点，跨 Agent **合并为一行**（来源展示如 `opencode+pi+codex`）  
- 合并时 **优先保留非空 apiKey**  
- 与 ModelHub 已有端点比对：标「端点已存在」  
- **名称唯一**：与已有同名或同批重名需改名或选覆盖  

#### 3.2.3 交互

| 行为 | 规则 |
|------|------|
| 默认勾选 | 仅「新」端点默认勾选；「端点已存在」默认不勾选 |
| 导入后刷新 | 已存在项（含刚导入）取消勾选；保留其余选择状态 |
| 动作 | 每行：导入（新建）/ 覆盖已有 / 跳过；名称可编辑 |
| 全选 | 「全选可导入」= 只勾不存在的 |
| 状态列「无 Key」 | 扫描时读不到密钥（OpenCode 仅 provider 无 options.apiKey 且 auth 无条目、Codex 自定义常无明文 key 等） |

### 3.3 Agent 绑定（会话草稿）

#### 3.3.1 数据生命周期

| 事件 | 行为 |
|------|------|
| 本会话首次进入绑定页（尚无草稿） | `read_live_bindings`：从磁盘解析并匹配到库内 Provider/Model，写入 `draftBindings` |
| 修改表单（模式 / Provider / Model 等） | **即时**写入 App 内存 `draftBindings`（不落盘、无「保存」步骤） |
| 切换侧栏 Tab 再回来 | **不重新读盘**，恢复内存草稿 |
| 重置 | 再读磁盘，覆盖草稿 |
| 关闭应用 | 草稿丢失 |
| 应用同步 | 使用内存草稿；请求体带 `bindings`；绑定页改动无需再点保存即可反映到 Diff / Apply |

**UI：** 仅提供 **重置** 按钮；**不提供**「保存绑定」；**不展示**「自上次重置后有修改」类脏状态提示。

#### 3.3.2 各 Agent 绑定含义

| Agent | 绑定内容 | 磁盘「真实配置」解读要点 |
|-------|----------|---------------------------|
| Claude | 官方 / 第三方；Provider + Model（可选 haiku/sonnet/opus） | `env.ANTHROPIC_BASE_URL` 空 → 官方；否则第三方 |
| Codex | 官方 / 第三方；Provider + Model；记录磁盘 `provider_key` 槽名 | `model_provider` 为 `openai` 或空 → 官方 |
| OpenCode | 默认 Provider + Model（+ 可选 small） | 优先 `opencode.json` 的 `model`；否则 `~/.local/state/opencode/model.json` 的 `recent[0]` |
| Pi | 默认 Provider + Model | `settings.json` 的 `defaultProvider` / `defaultModel` |

匹配策略：按 baseUrl（及协议宽松匹配）+ 上游 model id 对齐到库内记录；匹配失败显示「未匹配 / 未选择」。

**选择 Provider 后的默认 Model：** 切换 Provider 时，自动将该 Provider 下模型列表的 **第一项** 设为 Model；若无模型则 Model 为空。四个 Agent 均一致。

> OpenCode / Pi 绑定与 Apply 写入的是 **主模型（默认模型）**；OpenCode 还可设 `small_model`。  
> OpenCode 的 `opencode.json` **可以不写**顶层 `model`（用户常见状态），主模型可能只在 state 的 recent 里。

### 3.4 应用同步（Apply）

#### 3.4.1 流程

1. 选择要同步的 Agent（默认全选；可全选/清空）  
2. **更改对比**：磁盘现状 → 按当前会话草稿 Apply 后的预期  
3. 确认应用 → 写前备份 → 写出 → 结果与是否需重启  

#### 3.4.2 Diff 规则

- 展示字段级 diff：`=` 不变 / `+` 新增 / `-` 删除 / `~` 变更  
- **密钥**：比较真实 token 是否相同；相同则 `unchanged`，不同才 `changed`；禁止「有 key 就显示 updated」  
- Provider 写出 key：优先 **磁盘已有同 baseUrl 的 map key**（避免 `muapi` → `muapi-xxxx`）  
- 文案不写第三方产品名  

#### 3.4.3 分 Agent 写出规格

##### Claude Code

- 文件：`~/.claude/settings.json`  
- 第三方：写 `env.ANTHROPIC_BASE_URL`、`ANTHROPIC_AUTH_TOKEN`、可选 `ANTHROPIC_MODEL` / 分层默认、`model`  
- 官方：清除上述劫持相关 env 与 `model`  
- **不写** `_modelhub` 元数据字段  
- **不修改** 其它工具的「当前启用」数据库  

##### Codex（方案 B）

```toml
model_provider = "modelhub"   # 或绑定中的 providerKey，默认 modelhub
model = "<upstream model id>"

[model_providers.modelhub]
name = "..."
base_url = "..."
wire_api = "responses"
experimental_bearer_token = "sk-..."   # 中转 key；不改 auth.json
```

- **不修改** `~/.codex/auth.json`（保留官方登录缓存）  
- 不依赖 `env_key` / 不强制 `requires_openai_auth = true`  
- 其它 `[model_providers.*]`（如历史 custom）可保留；**运行时只看顶层 `model_provider`**  
- 需重启 Codex  

##### OpenCode

- 合并写入 `provider` 中 enabled 的 Provider（key 优先复用磁盘同 URL 的 id）  
- 设置顶层 `model`（及可选 `small_model`）为 `providerKey/modelId`  
- 密钥：写入 auth 约定或 options（实现以 adapter 为准）  
- 不破坏 mcp / plugin 等无关字段  

##### Pi

- 合并 `models.json` 的 providers（enabled）  
- `settings.json`：`defaultProvider` / `defaultModel`（defaultProvider 优先磁盘已有 key）  
- 不写无意义的 `_modelhub` 污染（当前 Pi adapter 已去掉 managed 标记写入）  

### 3.5 备份

- Apply 前按 Agent 备份目标文件到 `~/.modelhub/backups/<agent>/<timestamp>/`  
- 保留份数：`config.backupKeepCount`（默认 10）  
- UI：列表展示；一键恢复可后续增强  

### 3.6 设置

- 展示语言、备份份数、数据目录、各 Agent 路径检测结果  
- 路径覆盖编辑：后续可增强  

---

## 4. 协议与兼容

| protocol | 主要消费者 |
|----------|------------|
| `anthropic-messages` | Claude；部分 Pi/OpenCode |
| `openai-completions` | OpenCode、Pi、多数中转 Chat Completions |
| `openai-responses` | Codex（强相关）、部分 OpenCode（`@ai-sdk/openai`） |

- 分配给 Codex 且 protocol ≠ `openai-responses` 时 UI **警告**  
- 同一物理中转多协议：拆成多个 Provider 或接受宽松匹配  

---

## 5. 本地存储

```text
~/.modelhub/
  config.json       # 语言、备份份数、路径覆盖
  store.json        # providers / models / testPrompts / modelTestResults（及历史 agentBindings 可忽略）
  secrets.json      # secretRef → apiKey，0600
  backups/
    claude|codex|opencode|pi/
      <timestamp>/
```

| 数据 | 持久化 | 说明 |
|------|--------|------|
| Provider / Model / secrets | 是 | 库真相源 |
| testPrompts | 是 | 连通性测试提示词；含默认种子「连通性探测」 |
| modelTestResults | 是 | 每模型最近一次成功/失败摘要（ok / testedAt / latencyMs）；**不含**完整请求日志 |
| 连通性详细日志 / session | **否** | 前端内存（`lastTestResults` logs、single/batch/multi session）；关应用即失 |
| agentBindings 草稿 | **否** | 仅前端会话内存 + Apply 请求携带 |
| 各 Agent 配置文件 | 是（各产品） | Apply 的写出目标 |

---

## 6. Store 与 API 摘要

### 6.1 核心类型（逻辑）

- `Provider` / `Model` / `Secrets`  
- `TestPrompt` / `ModelTestResult` / `TestConnectionRequest` / `TestConnectionResult`  
- `AgentBindings`：claude / codex / opencode / pi  
- `ApplyRequest`：`agents: string[]` + 可选 `bindings: AgentBindings`  
- `ImportRequest`：`items: { id, name, action: import|override|skip }[]`  
- `ApplyPreview`：各 Agent 的 DiffLine 列表  

### 6.2 主要 Tauri 命令

| 命令 | 用途 |
|------|------|
| get_state | 库 + 路径检测 + 密钥遮罩 |
| Provider/Model CRUD、clone、enabled、delete 批量 | 库维护 |
| fetch_provider_models | 远程模型列表 |
| preview_import / run_import | 导入 |
| read_live_bindings | 从磁盘解析绑定 |
| preview_apply / apply_config | 对比与写出（带可选 bindings） |
| list_backups / reveal_api_key | 备份与密钥 |
| test_model_connection | 模型连通性测试；可选 `runId`/`timeoutSecs`；过程中 emit `test-connection-log` |
| list/upsert/delete_test_prompt | 测试提示词 CRUD（store.testPrompts） |
| set_default_test_prompt | 将指定提示词设为默认（其余取消默认） |
| record_model_test_result | 持久化某模型最近一次连通性测试结果（ok / testedAt / latencyMs） |

---

## 7. 页面信息架构

```text
ModelHub
├── 提供商      # 列表 + 详情（默认「模型」Tab，其次「连接」）
├── Agent 绑定  # 磁盘加载 + 会话草稿（即时）+ 重置
├── 应用同步    # 选 Agent + Diff + Apply（读会话草稿）
├── 导入
├── 备份
└── 设置
```

- 全局：右上角「应用更改」跳转应用同步  
- 提供商列表工具栏：多选后「测试所选」「删除所选」  
- 连通性测试弹窗关闭后 session 仍在内存；主导航 Tab 保活与测试 session 独立  
- 「测试所选」弹窗：提供商筛选 chip + 响应时间排序；「测试全部」弹窗：响应时间排序  
- 弹窗：Esc / 点遮罩关闭；删除类二次确认  
- Toast：右下角浮层  

---

## 8. 技术栈

| 层 | 选型 |
|----|------|
| 壳 | Tauri 2 |
| 前端 | React + TypeScript + Tailwind |
| 后端 | Rust（serde_json / toml / reqwest async） |
| 包管理 | pnpm |
| 数据 | `~/.modelhub` JSON |

### 建议源码结构

```text
ModelHub/
  docs/REQUIREMENTS.md
  src/                 # React
    pages/             # Providers Agents Apply Import Backups Settings
    components/        # Layout Modal Toast TestConnection* Batch* Multi*
    lib/               # single/batch/multi test sessions, lastTestResults, testDisplay, openExternal
    api/tauri.ts
  src-tauri/src/
    store/             # types + persistence
    adapters/          # claude codex opencode pi import live preview fetch_models
    commands.rs
    paths.rs backup.rs
```

---

## 9. 决策清单（现行）

| # | 决策 |
|---|------|
| 1 | 桌面 GUI：Tauri + React；跨 macOS / Windows / Linux |
| 2 | Provider-first；名称唯一；不同 key 可多实例 |
| 3 | Claude/Codex 只 Active；OpenCode/Pi 全量 enabled + Active |
| 4 | 密钥明文 secrets（0600）；UI 遮罩/显示/复制 |
| 5 | 无本地代理；重启提示代替热切换 |
| 6 | 只做模型配置 |
| 7 | 导入：端点合并（baseUrl+protocol）、名称冲突可改名/覆盖 |
| 8 | OpenCode key：`options.apiKey` 优先于 auth.json |
| 9 | OpenCode 主模型：config 无则读 `model.json` recent |
| 10 | Codex Apply：`experimental_bearer_token`，不改 auth.json |
| 11 | Agent 绑定：磁盘读 + 会话内存草稿，不持久化 store；**无保存按钮**，修改即时进草稿 |
| 12 | Apply 前 Diff；密钥仅当真变化才标 changed |
| 13 | 写出 Provider key 优先复用磁盘已有同 URL 的 key |
| 14 | 不向 Claude settings 写 `_modelhub` 元数据 |
| 15 | UI 不强调与其它配置工具的竞品对比文案 |
| 16 | 绑定页切换 Provider 时自动选中该 Provider 下第一个模型 |
| 17 | 绑定页切换 Tab 不重新读盘；仅「重置」重新读盘 |

---

## 10. 用户主流程

```text
1. （可选）导入本机配置 → 去重合并 → 补全 key / 改名
2. 提供商：维护模型、获取模型、启用同步（enabled → OpenCode/Pi 共用全量同步）
3. Agent 绑定：首次/重置读盘 → 调整（即时进会话草稿，选 Provider 自动选首模型）
4. 应用同步：看 Diff（基于草稿）→ 选 Agent → 确认写出 → 按提示重启
```

---

## 11. 已知限制与后续

| 项 | 说明 |
|----|------|
| Codex 与历史 custom 并存 | 正常；以 `model_provider` 为准 |
| 与其它切换工具的 UI 状态 | 不同步；以磁盘为准 |
| OpenCode / Pi 同步目录 | 当前共用全局 `Provider.enabled`，两端同步集相同（分 Agent 独立同步列表为后续可选，**未纳入当前需求**） |
| 备份一键恢复 | 可增强 |
| 设置路径编辑 | 可增强 |
| Keychain / 加密 | 可选后续 |
| 会话草稿跨重启 | 当前故意不做 |
| 连通性详细日志跨重启 | 故意不持久化；仅 `modelTestResults` 摘要落盘 |
| 连通性多会话并行 | 同时仅一个 multi session；进行中时不可用新勾选顶替；详情 batch 与 multi 对同一提供商互斥启动 |
| 提供商余额/额度 | **当前版本不做**；中转站无统一标准，且仅 Key 时常为 `unlimited_quota` 或需账号态，无法可靠展示「剩余额度」 |

---

## 12. 修订记录

| 日期 | 说明 |
|------|------|
| 2026-07-23 | 初版冻结（早期） |
| 2026-07-23 | **重写**：对齐实现（导入合并、会话绑定、Codex bearer、Diff、OpenCode state、写出 key 复用等） |
| 2026-07-24 | **同步已实现交互**：绑定无保存按钮/无脏提示、修改即时草稿、切换 Tab 不读盘、选 Provider 自动首模型；明确 OC/Pi 仍共用 enabled（独立同步列表未做） |
| 2026-07-24 | **模型测试连接**：模型行测试 + 可保存提示词（store.testPrompts）；入口仅模型行 |
| 2026-07-23 | **连通性测试增强**：可设默认提示词；超时；实时日志；关弹窗不中断；批量串行测全部；每模型日志；列表最近测试结果 |
| 2026-07-23 | **提供商 UX**：详情默认「模型」Tab；baseUrl 点开浏览器；Model ID 手输+远程选取；主 Tab 保活不丢本地状态 |
| 2026-07-23 | **最近测试持久化**：结果写入 store.modelTestResults；启动回填；悬停显示测试时间 |
| 2026-07-23 | **列表测试所选**：多提供商连通性测试，全局并发 3、同提供商串行 |
| 2026-07-23 | **测试状态跨入口共享**：列表 multi / 详情 batch / 单测共用日志与结果缓存；改勾选仍可续看进行中 multi 会话；详情可看到列表测过的结果 |
| 2026-07-23 | **Toast 收敛**：启用/禁用与测试完成不弹成功 Toast；失败与阻断仍提示 |
| 2026-07-23 | **文档回写**：连通性三入口、session 保活、日志仅内存、modelTestResults 持久化范围与命令表对齐现行实现 |
| 2026-07-23 | **批量列表 UX**：测试所选提供商筛选（全部+按提供商 chip）与响应时间排序；测试全部同排序；「最近测试」跟会话待测/进行中状态 |
| 2026-07-23 | **修复**：列表 multi 进行中打开详情测试全部黑屏（空 batch session）；最近测试不随新一轮待测刷新 |
| 2026-07-23 | **范围确认**：提供商余额/额度查询与展示不纳入当前版本（写入非目标与已知限制）；连通性相关规格已对齐实现，无需再改功能描述 |

# ModelHub — 产品需求与技术规格

> 状态：**现行实现规格**（随开发迭代更新，非早期冻结草稿）  
> 最后更新：2026-07-29（跨平台安全写入；恢复轮转保护；配置校验）
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
- 导入时按模型粒度勾选

---

## 2. 核心心智模型

```text
ModelHub 库（持久）
  Provider（名称唯一）
    └─ Models[]
  agentCatalogs（持久：OpenCode / Pi 各自的同步目录 = Provider + 可选模型子集）
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
   - OpenCode / Pi：**各自同步目录（`agentCatalogs`）写出所选 Provider 及其所选模型** + Active 主模型（两端**独立**，各自选 Provider 与模型子集）  
5. **两种生命周期分离**：  
   - **同步目录（catalog）** 是持久配置意图 → 落 `store.agentCatalogs`，勾选即落盘  
   - **Active / 模式** 是会话草稿 → 即时进内存，不写 store；首次/重置从磁盘读；切换 Tab 不丢（页面 keep-alive）  
   - OpenCode / Pi 的 Active 主模型 **仅能从各自同步目录内的 Provider 选**  
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
| notes | 备注 |

> Store 的 Provider/Model 只保存 UI 可配置字段；不保存 Agent 原生 `headers`、`compat`、模型 `reasoning` / 上下文等扩展配置。`enabled` 字段仅为向后兼容与首次迁移种子保留，不再有 UI 开关、不驱动写出；OC/Pi 写谁完全由 `agentCatalogs` 决定。

#### 3.1.2 操作

- 新建 / 编辑 / 删除（二次确认）；列表多选、全选、批量删除  
- **编辑保存后**：刷新密钥遮罩；详情「显示」缓存的明文密钥清空，需重新点「显示」以见最新 key（避免改 key 后仍显示旧值）  
- **列表搜索**：**仅匹配 Provider 名称**（大小写不敏感子串）；不搜 URL / 协议  
- **全选**：仅作用于**当前搜索结果（可见列表）**；勾选=并入可见 ID，取消=仅去掉可见 ID；筛选外已勾项保留（与导入页「清空当前」同语义）  
- **空态**：库为空 →「暂无提供商…」；库非空但搜索无命中 →「无匹配结果」  
- 删除提供商后：清勾选 / 远程模型缓存 / 详情选中；会话 `draftBindings` 中悬空 Provider/Model 引用在 `get_state` 刷新时 **scrub 掉**（避免 Apply 报「Provider 不存在」）  
- **克隆**：复制 URL/协议/模型，换名称与 key；失败时弹窗内错误、保持打开
- **无「启用同步」开关**：Provider 是否同步到 OC/Pi 改由 Agent 工作台的同步目录决定（§3.4.3）  
- **新建 / 克隆 Provider 默认加入 OC/Pi 两个同步目录**（`modelIds` 空 = 全部模型），恢复旧版「新增即默认同步」；不需要可在 Agent 工作台取消勾选  
- Toast：右下角浮层；连续 Toast **重置计时**（不互相提前关掉）；**测试完成/停止不弹成功 Toast**；失败与阻断必须提示  
- **主导航 Tab 保活**（visit-then-keep-alive）：切走再回来保留页面本地状态；Providers 页隐藏时 **暂停** test-session 订阅，回到页时再订阅并刷新展示  

#### 3.1.3 模型

- 详情 **默认打开「模型」Tab**（模型在前、连接在后）  
- 手动添加；可从远程列表多选添加（**后端 `add_models` 批量一次落盘**）  
- **从已获取列表选择**：进入时**不预勾**第一项；支持 **搜索 Model ID / 名称**；提供 **全选**（仅当前搜索结果，筛选外已勾保留）与「已选 N / M」；未勾选不可提交  
- **获取模型**：`{baseUrl}/models` 或 `{baseUrl}/v1/models`（async，「获取中…」，防重复点击）  
- 远程列表缓存按 **providerId** 隔离；**编辑 baseUrl/协议等连接信息后清空该 Provider 缓存**；删除 Provider 时同步清缓存  
- 编辑 Model ID / 展示名；Model ID 可手输，有远程缓存时可选取回填  
- **无「启用」开关**：模型只要在库中即可用/可测/可同步；同步哪些模型由 Agent 工作台同步目录的模型子集决定（§3.4.3）  
- **模型列表多选**：行勾选 + 全选；**删除所选**二次确认；后端 `delete_models` 批量一次落盘  
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
| 测试全部 | **串行**（测该 Provider 全部模型，无「仅已启用」过滤）；**停止**：待测立刻标跳过，当前请求跑完后结束会话；行内响应时间：优先本轮 `result.latencyMs`，否则回退 `modelTestResults.latencyMs`（重启后仍显示） |
| 测试所选 | **全局并发 3**，**同提供商串行**；**停止**：待测立刻标跳过，进行中请求跑完后结束会话；响应时间回退规则同「测试全部」 |
| 请求头 | 默认按协议自动注入客户端头（折叠行展示摘要）；弹窗可「自定义」写 `Key: Value` 覆盖/追加。合并：`协议默认 → 本轮覆盖`。协议默认：`anthropic-messages` → `User-Agent: claude-cli/2.1.79` + `x-app: cli` + `anthropic-beta: context-1m-2025-08-07`（兼容强制 1M 上下文的 Claude 中继）；`openai-completions` → `User-Agent: openai-node`；`openai-responses` → `User-Agent: codex_cli_rs/0.144.4`（兼容按 Codex 客户端标识放行的中继）。混测时各模型各自用协议默认，自定义为整轮全局覆盖。有覆盖时显示「清除覆盖」。仅本轮生效，不写回 Provider |
| 提示词区 | 已保存选择 / 内容 / 另存为名称 / 保存 归为同一「提示词」分组；另存为名称紧挨内容下方 |
| 跨入口共享 | `lastTestResults` + session；列表测过的结果可在详情看到 |
| 提示词 | `store.testPrompts`；默认种子「连通性探测」内容：`将123@qq.com转为Base64，直接回复结果`；可设默认/删（默认不可删）；**单测与批量弹窗均支持保存/设默认/删除** |
| 日志 | 脱敏；详细日志 **仅内存**；清空单测日志同步清共享缓存，避免批量结果回填 |
| 最近测试列 | 成功/失败/测试中/待测/跳过 + **响应时间（ms）**（有则紧挨徽章显示）；摘要 `modelTestResults` 落盘含 `ok` / `testedAt` / `latencyMs`，重启后 hydrate 恢复状态与耗时；完整日志仅内存 |
| 协议 | completions / responses / anthropic-messages；completions/anthropic 用小 `max_tokens`；**responses 不发送 `max_output_tokens`**（兼容拒绝该参数的第三方网关） |
| 触发 | **仅用户点击发送/开始** 才请求 |

### 3.2 模型一览（Models overview）

跨提供商的**选型工作台**：看哪些模型最近测通、谁响应更快。不替代提供商页的配置/CRUD。

#### 3.2.1 定位

| 项 | 行为 |
|----|------|
| 入口 | 主导航「模型一览」（`PageId=models`），位于「提供商」之后 |
| 行 | 库内每一个 Model（关联 Provider） |
| **可用** | **最近一次连通性测试成功**（与 `modelTestResults` / 内存 session 一致）；**不**展示、**不**用 OC·Pi 同步目录状态 |
| 响应时间 | 有则显示 ms（成功/失败都显示）；重启后从落盘摘要恢复 |
| 测试共享 | 与提供商页同一套 single/batch/multi session + `lastTestResults`；两边结果互通 |
| 非目标 | 本页不做模型增删改、不做同步目录编辑、不做用量/余额/历史曲线 |

#### 3.2.2 筛选 / 排序 / 摘要

- 搜索：Model ID / 展示名 / 提供商名  
- 连通性 **chips**（带数量）：全部 / 可用 / 失败 / 未测；数量随搜索·提供商·协议变化，**不含**状态 chip 自身  
- 提供商、协议筛选  
- 默认排序：可用优先 → 响应时间升序 → 失败 → 未测；另有耗时↑↓、最近测试、Model ID  
- 仅当 session 内确有 **running/排队 pending** 时显示「测试中 / 排队」；**未测 ≠ 进行中**  
- 可用中位耗时（基于当前搜索/提供商/协议范围下的可用行）  

#### 3.2.3 操作

- 行内「测试」：单模型弹窗（与提供商页相同）  
- **测当前列表** / **测所选**：复用 multi 批量会话（全局并发 3、同提供商串行）  
- 点击提供商名 → 跳转「提供商」并选中该 Provider  
- 全选当前筛选结果（勾选语义与列表可见集合一致）  

### 3.3 导入（Import）

#### 3.3.1 数据源与取 Key

| 源 | 配置 | Key 来源 |
|----|------|----------|
| OpenCode | `provider.*` | `options.apiKey` **优先**，其次 auth.json |
| Pi | `models.json` | `apiKey` |
| Claude | `settings.json` env | `ANTHROPIC_AUTH_TOKEN` / `API_KEY` |
| Codex | `[model_providers.*]` | ① `experimental_bearer_token` ② `requires_openai_auth` 时 `~/.codex/auth.json` 的 `OPENAI_API_KEY` ③ 兜底 auth.json 有 key 则用 |

扫描失败时 `scanNotes` 按源给出可读错误（解析失败等），不阻断其它源。

#### 3.3.2 合并与去重

- **同一 baseUrl + protocol** = 同一端点，跨 Agent **合并一行**（来源 `opencode+pi+codex`）  
- 合并时 **优先保留非空 apiKey**  
- **已存在**：同端点已在 ModelHub（**不比较模型集合**）  
- **名称**：全局唯一；冲突可改名 / 自动改名（`name-2` 递增避让）  

#### 3.3.3 覆盖语义（增量）

勾选已存在项 = **覆盖**：

1. 更新 name / baseUrl / protocol；notes 仅在原为空时补写来源  
2. Agent 原生扩展字段（如 `headers` / `compat` / 模型 `reasoning`）**不读取、不进入 Store**；后续 Apply 在磁盘原配置上按字段合并保留
3. **仅当扫描到非空 Key 时** 更新密钥（空 Key **不覆盖** 已有 secrets）  
4. **模型增量**：只添加库中尚未有的 model id；**不删除** 已有模型  

预览字段：`extraModelCount`、`newModelIds`（绿）、`existingModelIds`（灰）。

#### 3.3.4 默认勾选与动作

| 行类型 | 默认勾选 | 勾选后动作 |
|--------|----------|------------|
| 新 + 有 Key | ✓ | 导入（新建） |
| 新 + 无 Key | ✗ | 若勾选 → 导入（结果提示补 Key） |
| 已存在 | ✗ | 若勾选 → **覆盖（增量）** |
| 不勾选 | — | 跳过 |

无「动作」下拉：**不勾选 = 跳过**。

#### 3.3.5 筛选与批量勾选（精简）

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

#### 3.3.6 校验与确认

- 名称空 / 本批重名 / 与库冲突（**含覆盖行改名撞到其它 Provider**）：行内红字；失败时 **清筛选并滚到首个错误行**  
- 实时冲突检测与提交校验共用同一套规则；**仅勾选中的行**参与冲突（未勾选的已存在行不误报）  
- 自动改名：在当前名称根上 `-2` 递增，**不剥**名称末尾已有的 `-N`（避免 `gpt-4` → `gpt-2`）  
- 导入前 **ConfirmDialog**：新建数 / 覆盖数 / 预计 +模型数 / 无 Key 数  
- 导入中 `importing` 与扫描 `scanning` 分离  
- 导入失败：可能已部分落盘 → 刷新库 + **保留勾选/改名** 再扫（`keep` 模式），避免 UI 与磁盘不一致  

#### 3.3.7 导入后

- 单条 Toast 摘要（含无 Key 提示）  
- 结果条：无 Key 列表（导入时 **id+名称快照**，二次扫描丢项仍可显示）+ **查看提供商**  
- 刷新预览（`clear` 模式：保留改名、全部取消勾选）  

#### 3.3.8 状态模型与保活

- 前端单源：`ImportItem = ImportPreviewItem + selected + error`（无 preview/rows 双表）  
- 重扫合并模式：`defaults`（默认勾选）/ `keep`（保留勾选与改名）/ `clear`（保留改名、全不勾）  
- **Tab 保活**：再次进入时若库（providers/models）相对上次扫描已变，**静默 `keep` 重扫**，避免陈旧「已存在/可补」  

#### 3.3.9 行 UI

- 卡片行：名称可编辑、状态 badge、协议缩写、模型展开  
- 展开模型列表：**绿色** = 将新增，**灰色** = 库内已有（跳过）；无图例文案  
- 无 Key 行略透明  
- 行空白点击切换勾选  
- 名称冲突显示「自动改名」（实时 warning + 提交后行内红字）；用户改名/勾选变化后 **按当前状态重算**：已修好的行消红，仍无效的行保持标红（不会一次清掉全部）  

组件：`ImportPage.tsx` + `ImportRow.tsx`。

### 3.4 Agent 工作台（绑定 + 应用同步，合并页）

「Agent 绑定」与「应用同步」合并为**单页 master-detail**：左侧 Agent 列表（含状态点），右侧选中 Agent 的详情 + 更改对比；右侧底部常驻「应用此 Agent」操作栏，详情滚动时仍保持可见。不提供跨 Agent 批量应用。

#### 3.4.1 两类持久性（关键）

| 概念 | 例子 | 存储 | 生命周期 |
|------|------|------|----------|
| **模式 + 默认模型（绑定）** | Claude 第三方 / OC 默认 B/gpt-4 | 会话草稿 `draftBindings`（**不落盘**） | 每次会话；首次/重置读磁盘 |
| **同步目录（catalog）** | OC 写出 A、B；Pi 写出 B、C | `store.json` 的 `agentCatalogs`（**持久**） | 长期配置；勾选即落盘 |

分开的理由：默认模型以磁盘为准、每次会话读；同步目录是长期意图，不该每次重置。

#### 3.4.2 绑定草稿生命周期

| 事件 | 行为 |
|------|------|
| 本会话首次进入（无草稿） | `read_live_bindings` 读磁盘并匹配库内 Provider/Model |
| 修改模式/默认模型 | **即时**写入 `draftBindings`（无保存按钮） |
| 切换 Tab | 不重新读盘（keep-alive） |
| 重置 | 「从磁盘重置草稿」再读磁盘覆盖（**不影响** catalog） |
| 库变更刷新（删除 Provider/Model 等） | `scrubBindings`：去掉草稿中已不存在的 providerId/modelId |
| 关闭应用 | 草稿丢失（catalog 保留） |
| 应用同步 | 请求携带 `bindings`；写出 catalog 从 store 读 |

#### 3.4.3 同步目录（OC / Pi）

- **结构**：`agentCatalogs.{opencode,pi}` 是 `CatalogEntry[]`，每项 `{ providerId, modelIds }`。`modelIds` **空 = 该 Provider 全部模型（动态，自动含新增）**；非空 = 仅同步指定模型  
- 仅 OC/Pi 详情显示；勾选 Provider / 展开选模型，**任何改动即调 `set_agent_catalog` 落盘并刷新库**  
- **选模型子集**：Provider 行可展开其模型多选。全勾（= 全部）存空 `modelIds`（保持动态）；取消勾到 **零个模型 = 该 Provider 移出目录**（避免「空=全部」歧义）  
- 搜索（名称子串）+ **Provider 全选仅作用当前搜索结果**（筛选外已勾保留，与提供商页同语义）  
- 删除 Provider 时从两个 catalog 列表 scrub 掉该项；`set_agent_catalog` 落盘时去除悬空 provider / model id、按 Provider 去重并保序（模型 ID 不额外去重）
- **迁移**：旧 `store.json` 无 `agentCatalogs` 时，用当时 `enabled=true` 的 Provider 同时种子进 opencode/pi 两列表（`modelIds` 空 = 全部，行为与旧全局 `enabled` 一致）；一旦为 `Some`（即便空列表）不再重新种子。旧版 catalog 的裸 providerId 字符串仍可反序列化（→ 空模型子集 = 全部）  
- **默认模型限定在同步目录内**：OC/Pi 的默认 Provider 只能从已加入 catalog 的 Provider 选  
- **新建 / 克隆 Provider 自动加入两个 catalog**（空模型子集 = 全部）；catalog 尚为 `null`（未迁移）时不追加，留待首次 load 迁移种子  

#### 3.4.4 各 Agent 磁盘解读

| Agent | 绑定 | 磁盘解读要点 |
|-------|------|----------------|
| Claude | 官方/第三方；Provider+Model | BASE_URL 空 → 官方 |
| Codex | 同上；记录 `providerKey` | `model_provider` 为 openai/空 → 官方；第三方块回匹配 Store 时，OpenAI 系协议将末尾 `/v1` 视为等价，避免 Apply 自动补 `/v1` 后重启显示未匹配 |
| OpenCode | 默认 Provider+Model（+small） | config `model` 优先，否则 `model.json` recent[0]；受管 Provider 优先按 `_modelhub.providerId` 回匹配 Store |
| Pi | 默认 Provider+Model | settings `defaultProvider` / `defaultModel`；受管 Provider 优先按 models.json 的 `_modelhub.providerId` 回匹配 Store，避免写出补 `/v1` 后 URL 文字差异导致重启丢绑定 |

切换 Provider 时 **自动选中该 Provider 下第一个模型**。

#### 3.4.5 应用（Apply）

- **状态点**：进入/草稿/库变化时一次 `preview_apply([], draft)` 拿全部四个 Agent 的 Diff；某 Agent Diff 含非 `same` 行 → 标「有更改」，否则「一致 / 无配置」  
- **右侧 Diff**：只显示当前选中 Agent（磁盘现状 → Apply 后）  
- **应用此 Agent**：仅写出当前选中的 Agent；按钮位于右侧详情底部常驻操作栏，不随配置与 Diff 内容滚动
- 流程：备份 → 写出 → 行内结果 / 重启提示  
- Agent 原生配置读取或解析失败时，Preview 明确报错并清空旧预览，不把损坏文件当空配置。

#### Diff 规则

- `=` / `+` / `-` / `~`  
- 密钥比较真实 token（界面只显示 `***`）；相同则 unchanged  
  - **Claude / Codex**：比磁盘 token 与 Store 密钥（有则 `+/***` / `~ *** (changed)` / `= unchanged`）  
  - **OpenCode**：对同步目录内每个 Provider，比 `auth.json[<writeKey>]`（或匹配块旧 key / 遗留 `options.apiKey`）与 Store 密钥  
  - **Pi**：对同步目录内每个 Provider，比磁盘块 `apiKey` 与 Store 密钥  
  - 仅 Key 变化时，四个 Agent 的更改对比与状态点均会标「有更改」  
- **字段所有权**：ModelHub 只覆盖 UI 可配置字段及必要的 Agent 映射字段；Agent 原生未知字段不读入 Store、不在 Apply 中凭空重建或清空
- **Codex 保留式写出**：只更新 `model` / `model_provider` 与所选 `model_providers.<key>` 的名称、base_url、wire_api、token；该块其它字段及其它 Provider 块全部保留；官方模式只切 `model_provider=openai`
- 写出 Provider key 由 **Provider 名称 slug** 生成（名称全局唯一），写出集内去重（撞名加 `-2`）；**不复用磁盘已有 key**。同一 baseUrl 不同协议的两个 Provider（如 `jianzhile` responses + `jianzhile-cc` anthropic）各得独立 key，不再串块/互相覆盖。Apply 与 Preview 用同一份 key 映射，Diff 与实际写出一致  
- **OC/Pi 模型级 Diff**：对每个同步 Provider，比对磁盘该块已有模型 id 与本次将写出的集合，逐条列出 `+ 新增模型` / `- 不再同步模型`（相同的省略）  
- **OC/Pi 匹配与保留**：写出 Provider 优先按 `_modelhub.providerId` 匹配磁盘原块（改名/slug 变化仍可继承），找不到时按本次目标 key 匹配。匹配项以磁盘原配置为基础重建：ModelHub 管理字段覆盖，Provider/模型的其它原生扩展字段保留
- **OC/Pi 目录清理**：仅删除退出同步目录的 `_modelhub.managed=true` 受管块；未受 ModelHub 管理的本机/第三方 Provider 块原样保留
- **OC/Pi 默认模型条件写出**：Apply 仅在绑定草稿 provider+model **均已选** 时才写 OpenCode `model` / Pi `defaultProvider`+`defaultModel`；草稿未选默认时 **不改** 磁盘现值。Preview 与此对齐——草稿未选时该行显示磁盘现值为 unchanged，而非「→ —」的假变更  
- **baseUrl `/v1` 规范化**：Apply 写出 Codex/OC/Pi 的 baseUrl 时，openai 系协议（completions / responses）自动补 `/v1`（已有则不重复），anthropic 保持裸 URL。与连通性测试的 `api_root` 及取模型行为一致——**测试能通过的模型 Apply 后也能用**

#### 写出规格摘要

| Agent | 要点 |
|-------|------|
| Claude | env + model；**不写** `_modelhub` |
| Codex | 更新当前槽的名称 / base_url / wire_api / `experimental_bearer_token`，保留槽内其它字段与其它 Provider 块；**不改** auth.json；官方模式只切活动 Provider |
| OpenCode | **catalog(opencode)** 控制 ModelHub 受管块；未受管 Provider 保留。匹配块保留原 `options`、headers 和模型扩展字段，ModelHub 仅覆盖 `npm` / 名称 / baseURL / 同步模型 ID·名称 / `_modelhub`；Key 统一写 `auth.json` 并移除可能优先使用的旧 `options.apiKey`；`mcp`/`plugin` 等其它顶层键不动 |
| Pi | **catalog(pi)** 控制 ModelHub 受管块；未受管 Provider 保留。匹配块保留 headers、compat、模型 reasoning/上下文等扩展字段，ModelHub 仅覆盖 baseUrl / api / apiKey / authHeader / 同步模型 ID·名称 / `_modelhub`；新 Provider 或原块缺少 UA 时补 `User-Agent: pi-coding-agent`，已有 UA 保留；settings 其它键不动 |

> **只管理自己的部分**：OC/Pi 写出块带 `_modelhub.managed` 标记。取消同步时只清理此类受管块，不删除用户/其它工具的未受管 Provider。同步目录内按 `_modelhub.providerId` 优先、目标 key 兜底匹配磁盘块，并保留 ModelHub 未管理的 Provider/模型原生字段。OpenCode 的 `auth.json` 为追加式（只加不删），可能残留无用 key，无害。

### 3.6 备份

- Apply 前自动备份到 `~/.modelhub/backups/<agent>/<timestamp>/`
- **同一 Agent 一次 Apply 共用一个时间戳目录**；OpenCode / Pi 等多文件写出归入同一快照组
- 时间戳为 UTC `YYYYMMDD-HHMMSS-mmm`（毫秒），避免同秒连续 Apply 互相覆盖；历史秒级目录仍可读
- `backupKeepCount` 默认 10：**每个 Agent 独立**保留最近 N 组快照目录（不是全局总数）
- **备份页**：
  - 按 `agent + stamp` 分组展示快照（不再按单文件平铺）
  - Agent 筛选（全部 / Claude Code / Codex / OpenCode / Pi）与数量
  - 概览：快照组数、文件数、各 Agent 最近备份时间
  - 展开查看目录与文件路径；可复制单个文件路径、在系统文件管理器中打开快照位置（不提供快照目录复制按钮）
  - 支持单选/多选快照组删除：每行可勾选；「全选当前」只作用于当前 Agent 筛选结果，筛选外已选项保留；二次确认后永久删除所选 `agent + stamp` 目录，不修改 Agent 当前 live 配置；删除成功后刷新列表、数量和最近备份概览
  - keep-alive：重新进入页面自动刷新；手动「刷新」仍可用
- **一键恢复**（按快照组，非单文件）：
  - 确认后调用 `restore_backup(agent, stamp)`
  - 按**当前** Agent 配置路径写出（含 `config.paths` 覆盖；不是写回备份时的旧路径）
  - 文件名映射：Claude `settings.json`；Codex `config.toml`；OpenCode `opencode.json`/`opencode.jsonc`→当前主配置、`auth.json`；Pi `models.json` / `settings.json`
  - 恢复前先把将被覆盖的现有 live 文件再备份一组（新 stamp，计入保留策略）
  - 创建恢复前安全备份触发轮转时，正在恢复的源内容必须受保护；即使选择已满保留策略中的最旧快照也可完成恢复
  - 无法识别的备份文件跳过并在结果消息中说明；快照内无任何可恢复文件则失败
  - Claude / Codex 提示建议重启；OpenCode / Pi 一般不必
- **本版不做**：手动创建备份、单文件级删除/恢复、恢复后自动改 Store/绑定草稿

### 3.7 设置

- **可编辑**：备份保留份数（`backupKeepCount`，默认 10，合法范围 1–50；**每个 Agent 独立**保留最近 N 组快照）。通过 `set_backup_keep_count` 保存到 `~/.modelhub/config.json`，前后端均校验范围
- **只读展示**：语言（目前仅简体中文）、数据目录 `~/.modelhub`（复制 / 在文件管理器中显示）
- **Agent 路径**：展示当前检测结果（存在状态 + 完整路径；复制 / 打开位置）；**路径覆盖编辑本版不做**
- 关于：版本号与产品定位一句话

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
  store.json     # providers, models, agentCatalogs, testPrompts, modelTestResults
  secrets.json   # 0600
  backups/
```

| 数据 | 持久化 |
|------|--------|
| Provider / Model / secrets | 是 |
| **agentCatalogs（OC/Pi 同步目录）** | **是**（分 Agent 的 `{ providerId, modelIds }[]`；`modelIds` 空=该 Provider 全部模型；`null`=未迁移，首次 load 从旧 `enabled` 种子） |
| testPrompts / modelTestResults | 是（测试摘要，无完整日志） |
| 连通性详细日志 / test session | **否**（内存） |
| agentBindings 草稿（模式 + 默认模型） | **否**（内存 + Apply 请求） |

用户密钥与 Agent 配置在 **家目录**，不在应用仓库内。

---

## 6. Store 与 API 摘要

### 6.1 类型

- `Provider`（name/baseUrl/protocol/notes 等 UI 字段）/ `Model`（modelId/displayName）/ `Secrets`；不含 Agent 原生 headers/compat/capabilities
- `TestPrompt` / `ModelTestResult` / `TestConnectionRequest|Result`  
- `AgentBindings`（会话草稿：模式 + 默认模型）  
- `AgentCatalogs`：`{ opencode: CatalogEntry[] | null, pi: CatalogEntry[] | null }`（持久化同步目录）  
- `CatalogEntry`：`{ providerId, modelIds: string[] }`；`modelIds` 空 = 该 Provider 全部模型（动态），非空 = 指定子集  
- `ApplyRequest`：`agents` + 可选 `bindings`  
- `ImportPreview`：`items` + `scanNotes`  
- `ImportPreviewItem`：含 `modelIds` / `extraModelCount` / `newModelIds` / `existingModelIds` / `hasApiKey` 等  
- `ImportRequest`：`items: { id, name, action }[]`  

### 6.2 主要命令

| 命令 | 用途 |
|------|------|
| get_state | 库 + 路径 + 密钥遮罩 |
| Provider/Model CRUD、clone、delete 批量 | 库维护；Provider/Model 批量删除均单次 load+save（无 `set_provider_enabled`；`Model` 无 `enabled`） |
| set_backup_keep_count | 保存每 Agent 备份保留份数（1–50）；不开放整份 AppConfig 覆盖 |
| set_agent_catalog | 保存某 Agent（opencode/pi）同步目录 `CatalogEntry[]`（按 Provider 去重、去悬空 provider/model；模型 ID 不额外去重；单次落盘） |
| add_models | 批量添加模型（单次 load+save） |
| delete_models | 批量删除模型（单次 load+save） |
| fetch_provider_models | 远程模型列表 |
| preview_import / run_import | 导入 |
| read_live_bindings | 磁盘绑定 |
| preview_apply / apply_config | Diff 与写出 |
| list_backups / delete_backups / restore_backup / reveal_api_key | 备份列表、快照批量删除、恢复与密钥；`delete_backups` 先校验并去重全部 `{agent, stamp}` 再删除 |
| test_model_connection | 连通性测试 + 日志 event |
| list/upsert/delete_test_prompt / set_default_test_prompt | 提示词 |
| record_model_test_result | 最近测试摘要 |

---

## 7. 页面信息架构

```text
ModelHub
├── 提供商      # 列表 + 详情（默认模型 Tab）；测试全部 / 测试所选
├── 模型一览    # 跨提供商连通性/响应时间选型（不展示 OC·Pi 同步目录）
├── Agent 绑定  # 合并工作台：左 Agent 列表(状态点) + 右详情(模式/目录/默认模型 + 本 Agent Diff) + 右侧底部常驻单 Agent 应用栏
├── 导入        # 扫描 / 筛选 / 勾选 / 确认
├── 备份
└── 设置
```

- 全局页头不提供应用快捷按钮；通过左侧导航进入 Agent 工作台
- 弹窗：Esc / 遮罩关闭；删除二次确认  
- **ConfirmDialog**：Enter 确认、Esc 取消（处理中忽略）；确认钮默认聚焦  
- Toast：右下角  

### 建议源码结构

```text
src/
  pages/          Providers Models AgentWorkbench Import Backups Settings
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
| 3 | Claude/Codex 只 Active；OpenCode/Pi 写出**分 Agent 同步目录（Provider + 可选模型子集）** + Active（目录持久，各 Agent 独立） |
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
| 14 | OC/Pi 写出 key 由 Provider 名称 slug 生成（全局唯一）+ 写出集内去重（`-2/-3`）；apply 与 preview 共用同一映射，同 baseUrl 不同协议的 Provider 不再串块 |
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
| 25 | 导入只读取 UI 管理字段；Agent 原生 headers/compat/模型 capabilities 不进入 Store；失败后 keep 重扫 |
| 26 | 添加模型：远程列表不预选、可全选；批量 `add_models` |
| 27 | 详情模型列表多选 + 批量 `delete_models`；ConfirmDialog Enter 确认 |
| 28 | 编辑 Provider 保存后清空详情明文密钥缓存 |
| 29 | **模型一览**页：跨 Provider 连通性/耗时选型；与提供商页共享测试结果；不展示同步目录 |
| 30 | OpenCode/Pi **分 Agent 同步目录**：`store.agentCatalogs.{opencode,pi}` 持久化 `CatalogEntry[]`（providerId + 可选 modelIds 子集，空=全部动态）；旧库首次加载从全局 `enabled` 迁移种子；`set_agent_catalog` 落盘 |
| 31 | 「Agent 绑定」与「应用同步」**合并为一个 master-detail 工作台**；OC/Pi 默认模型限定在同步目录内；一次 `preview_apply` 驱动状态点与各 Agent Diff |
| 32 | **移除 `Model.enabled`**：模型只要在 Provider 下即可用；测试弹窗去掉「仅测已启用」过滤 |
| 33 | **移除 Provider 启用开关与 `set_provider_enabled` 命令**：同步范围完全由各 Agent 同步目录决定；`Provider.enabled` 字段仅保留供旧库迁移种子 |
| 34 | **只管理 UI 字段**：Apply 在 Agent 磁盘原配置上按字段合并；Codex/OC/Pi 保留未知原生字段与未受管 Provider，OC/Pi 仅清理 `_modelhub.managed` 且退出同步目录的块 |

---

## 10. 用户主流程

```text
1. （可选）导入：刷新扫描 → 筛选 → 勾选 → 确认 → 补 Key
2. 提供商：维护模型、获取模型、连通性测试
3. Agent 绑定（合并工作台）：选 Agent → 调 Active/模式（草稿）或勾同步目录（落盘）→ 看本 Agent Diff → 使用底部常驻栏应用此 Agent → 按需重启
```

---

## 11. 已知限制与后续

| 项 | 说明 |
|----|------|
| Codex 与历史 custom 并存 | 正常；运行时只看 `model_provider` |
| 与其它切换工具 UI | 不同步；以磁盘为准 |
| OC/Pi 同步目录 | 已分 Agent 独立列表（`agentCatalogs`）；旧库首次加载从全局 `enabled` 种子迁移 |
| `Provider.enabled` 字段 | 仅保留供旧库迁移种子；Providers 页启用开关、`set_provider_enabled` 命令已移除；同步范围由各 Agent 同步目录决定 |
| 会话草稿 / 测试详细日志 | 跨重启故意不持久化；草稿会随库删除 scrub |
| 导入 keep-alive 静默重扫 | 以 store 指纹为准；仅 Agent 配置文件变更需点「刷新扫描」 |
| 手动创建备份、单文件级删除/恢复、设置路径覆盖编辑、多语言 | 可增强；快照删除、一键恢复与备份份数编辑已支持 |
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
| 2026-07-24 | 编辑 Provider 后刷新密钥显示缓存；模型列表全选/批量删除（`delete_models`）；ConfirmDialog 支持回车确认 |
| 2026-07-24 | 新增「模型一览」：全局模型表、可用性/耗时筛选排序、测当前列表/测所选、跳转提供商；测试结果与提供商页共享 |
| 2026-07-24 | 模型一览筛选：状态 chips+计数；去掉推荐视图；修正未测被计为「进行中」 |
| 2026-07-24 | 连通性请求头：后端按协议自动合并默认客户端头；multi 混测不再共用一份 Claude UA |
| 2026-07-25 | 连通性弹窗：提示词区合并（另存为名称下移）；请求头改为自动摘要 + 可选自定义，去掉「填说明」 |
| 2026-07-25 | anthropic 连通性默认增加 `anthropic-beta: context-1m-2025-08-07`（anyrouter 等 1M 强制中继） |
| 2026-07-27 | OC/Pi 分 Agent 同步目录：store 新增 `agentCatalogs`（持久，`null` 首次从旧 `enabled` 迁移种子）+ `set_agent_catalog` 命令；apply/preview 改用各 agent 目录；删 Provider 同步 scrub 目录。「Agent 绑定」与「应用同步」合并为单页工作台（左 Agent 列表状态点 + 右详情：模式/目录/默认模型 + 本 Agent Diff + 应用此 Agent；底部一键应用全部有变更）。OC/Pi 默认模型限定在同步目录内 |
| 2026-07-27 | 同步目录升级为 **Provider + 模型子集**（`CatalogEntry{providerId, modelIds}`；`modelIds` 空=该 Provider 全部模型动态）；`set_agent_catalog` 接受 `CatalogEntry[]`，兼容旧裸字符串反序列化。**移除 `Provider.enabled` / `Model.enabled` 的 UI 与 `set_provider_enabled` 命令**：哪些 Provider/模型同步完全由 catalog 决定；测试弹窗去掉「仅已启用」过滤。OC/Pi 更改对比新增 **模型级 `+/-` diff**（对比磁盘该 Provider 块已有模型集） |
| 2026-07-27 | OC/Pi 更改对比新增 **孤儿块显示**：磁盘上不在同步目录的 Provider 块逐条列出。**OC/Pi 改为完全覆盖 provider 目录**：Apply 时清空磁盘上所有 provider 块（含用户/其它工具手写的）再只写同步目录，孤儿块在 Diff 中一律标「将删除」；Pi 写出块补齐 `_modelhub.managed` 元数据标记（与 OpenCode 对齐） |
| 2026-07-27 | 修复同 base_url 不同协议的 Provider（如 `jianzhile` responses + `jianzhile-cc` anthropic）写出 key 冲突：改用 **provider 名称 slug + 集合内去重**（`assign_catalog_write_keys`）取代「按 baseUrl 复用磁盘 key」，apply/preview 共用同一映射；避免 Diff 串块与 Apply 互相覆盖丢数据 |
| 2026-07-27 | 修复移除 `enabled` 后的回归：**新建 / 克隆 Provider 自动加入 OC/Pi 两个同步目录**（空模型子集=全部），恢复「新增即默认同步」；catalog 未迁移（`null`）时不追加，留待首次 load 种子 |
| 2026-07-27 | 修复 OC/Pi Diff 幻影变化：Apply 仅当绑定草稿的默认 Provider+Model 都已选时才写 `model`/`defaultProvider`/`defaultModel`，未选则不碰磁盘；Preview 对齐此行为——草稿未选时默认模型行回退磁盘现值显示 unchanged，不再显示「→ —」的假变更（导致 Apply 后仍标「已更改」） |
| 2026-07-27 | 修复「测试通过但 Apply 后请求失败」：OC/Pi 写出 baseUrl 对 **openai 系协议（completions/responses）自动补 `/v1`**（`agent_write_base_url`），与连通性测试 `api_root`、原生网关配置一致；anthropic 保持裸 URL。此前库中缺 `/v1` 的 Provider 测试走 `/v1` 能通、Apply 写裸 URL 打到网站首页 → `Stream ended without finish_reason`。另：Pi 写出块默认注入 `User-Agent: pi-coding-agent`（provider.headers 可覆盖） |
| 2026-07-28 | 对齐 `set_agent_catalog` 现行清理语义：按 Provider 去重并去除悬空 Provider/模型引用，模型 ID 本身不额外去重。 |
| 2026-07-28 | OC/Pi Apply 从“整块重新生成”改为“目录范围覆盖、匹配项保留式合并”：同步目录外 Provider 仍删除；同步目录内按 `_modelhub.providerId` 优先、目标 key 兜底匹配，保留 Provider/模型未知原生字段，Pi `headers` / `compat` 按键合并，ModelHub 管理字段继续覆盖。Preview 使用相同匹配规则。 |
| 2026-07-28 | 连通性测试按 OpenAI 协议区分客户端标识：completions 保持 `openai-node`，responses 默认改为 `codex_cli_rs/0.144.4`；本轮自定义 headers 仍可覆盖。 |
| 2026-07-28 | 收紧字段所有权：Store/导入移除不可在 UI 配置的 Provider headers/compat 与模型 capabilities；测试仅合并协议默认头和本轮覆盖。Codex/OC/Pi Apply 改为只覆盖受管字段，保留匹配块原生扩展字段；OC/Pi 未受管 Provider 保留，仅删除退出目录的 `_modelhub.managed` 块。Pi 新块或缺 UA 时补 `pi-coding-agent`，已有 UA 不覆盖。 |
| 2026-07-28 | 修复 OC/Pi 重启后默认模型显示“未匹配/未选择”：磁盘回读优先使用写出块 `_modelhub.providerId` 匹配 Store，再回退 URL/协议/名称；避免 Store 裸 baseUrl 与磁盘自动补 `/v1` 后无法匹配。 |
| 2026-07-28 | 备份页重构为**快照浏览器**：按 agent+stamp 分组、Agent 筛选、复制路径/打开位置、重新进入自动刷新；同次 Apply 多文件共用时间戳，时间戳升至毫秒防同秒覆盖。一键恢复仍不做。 |
| 2026-07-28 | **备份一键恢复**：`restore_backup(agent, stamp)`；确认弹窗；恢复前安全备份当前 live 文件；按当前检测路径映射已知配置文件名；未知文件跳过。删除/手动创建仍不做。 |
| 2026-07-28 | **设置页**：备份保留份数可保存；数据目录 / Agent 路径只读 + 复制/打开；语言暂仅中文。**OC/Pi 更改对比补密钥 Diff**（OpenCode `auth.json` / 遗留 inline key，Pi 块 `apiKey`）；仅 Key 变化也会标有更改。OpenCode 默认 `model` Preview 与 Apply 对齐（未选绑定时回退磁盘值）。 |
| 2026-07-29 | 修复跨平台写入与恢复可靠性：Store、Agent JSON、Codex TOML 和恢复统一使用同目录临时文件替换，Windows 支持覆盖已有目标；恢复源在安全备份轮转前读入保护，最旧快照不会因轮转导致恢复失败；轮转删除错误不再静默忽略。Preview 对损坏/不可读 Agent 配置明确失败并清空旧结果。Provider 批量删除改为单次读写，Store/Secrets 联合写入失败时回滚 Secrets。设置 API 收窄为 `set_backup_keep_count` 并在后端强制 1–50。移除已废弃的绑定落盘命令与无调用接口。 |
| 2026-07-29 | 备份页移除用途重复的快照级「复制目录」按钮；保留「打开位置」以及详情中的单文件「复制路径」。 |
| 2026-07-29 | 备份页新增快照组单选/多选删除：「全选当前」遵循当前 Agent 筛选且保留筛选外选择；二次确认后调用 `delete_backups(items)` 永久删除整组备份，不影响 Agent 当前配置；后端先对全部目标做 Agent 白名单、路径段、存在性校验和去重。 |
| 2026-07-29 | 修复 Codex 第三方 Provider 回读匹配：OpenAI completions/responses 的 Store 裸 baseUrl 与磁盘自动补 `/v1` 地址视为等价；避免 `model_provider=custom` 明明对应库内 Provider/Model，却在 Agent 工作台显示未匹配、Preview 误报 Provider `?` 与 Key 缺失。 |
| 2026-07-29 | 收敛 Agent 应用入口：移除全局页头「应用更改」和工作台「应用全部更改」；仅保留逐 Agent 应用，并将「应用此 Agent」放入右侧详情底部常驻操作栏，滚动配置/Diff 时持续可见。 |

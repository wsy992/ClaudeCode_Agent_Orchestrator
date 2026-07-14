# ClaudeCode Agent Orchestrator — 源码级架构研究

<p align="center">
  <b>对 ClaudeCode 多 Agent 调度核心的深度源码分析</b><br>
  7 个核心模块 · 63 个测试用例 · 4 万字研究报告 · 与 AutoGen / MetaGPT 多维对比
</p>

---

## 📖 研究背景

2024 年以来，以 ClaudeCode 为代表的 AI Agent 编程工具正在改变软件开发的方式——它们不再只是"代码补全"，而是能自主理解需求、规划步骤、执行命令并处理错误的智能体。然而，当一个 AI Agent 拥有 bash 权限、文件读写能力和网络访问能力时，如何保证它不会越界？这是一个当前 AI 工程化领域尚未被充分回答的问题。

本项目选择 **ClaudeCode 的 Agent Orchestrator（多 Agent 调度器）** 作为切入点，以 **源码级分析** 的方式，回答三个核心研究问题：

- **AI Agent 的运作机制**：ClaudeCode 的"Agent"究竟是如何运作的？它与单纯的 LLM API 调用之间的本质差异是什么？
- **多 Agent 协作模式**：多个 Agent 之间如何协调？复杂任务如何拆解为不同角色的 Agent 协作流水线？
- **安全边界设计**：当 AI 拥有 shell 权限时，权限模型和 Hook 拦截如何层层设防？

本研究覆盖 Agent Orchestrator 的 7 个核心源文件，编写 63 个测试用例，完成 4 万字研究报告，并与 AutoGen、MetaGPT 等主流多 Agent 框架进行多维度对比分析。

---

## 📁 项目结构

```
├── README.md                                               # 本文件
├── ClaudeCode_Agent_Orchestrator_研究报告_正式版.md        # 完整研究报告（4 万字）
├── ClaudeCode.pptx                                         # 汇报 PPT
│
├── claudecode-main/                                        # ClaudeCode 原始源码
│   └── rust/crates/runtime/src/
│       ├── bootstrap.rs          (12 阶段引导系统)
│       ├── permissions.rs        (5 级权限控制模型)
│       ├── hooks.rs              (Pre/Post 双拦截 Hook)
│       ├── session.rs            (版本化 JSON 会话管理)
│       └── conversation.rs       (对话引擎与消息路由)
│
└── source_code/                                            # 研究用源码 & 测试
    ├── conversation_core.rs                                # ConversationRuntime 核心引擎
    ├── agent_tools.rs                                      # 子 Agent 工具执行器
    ├── permissions.rs                                      # 权限系统复现与实验
    ├── hook_system.rs                                      # Hook 系统复现与实验
    ├── bootstrap.rs                                        # Bootstrap 系统复现
    ├── session.rs                                          # 会话结构实现
    ├── experiments.rs                                      # 扩展实验
    ├── test_suite.rs                                       # 63 个测试用例
    └── README.md                                           # 源码索引与编译指南
```

---

## 🏗️ 架构全景

### 分层设计方案

Agent Orchestrator 在 ClaudeCode 整体架构中处于中枢位置——它连接上层的 CLI/GUI 交互入口与底层的 LLM API、工具执行器，是理解 ClaudeCode 的关键切入点：

```
┌──────────────────────────────────────────────────────┐
│                    CLI / GUI 入口                       │
├──────────────────────────────────────────────────────┤
│                Agent Orchestrator                      │
│  ┌──────────┐  ┌──────────┐  ┌────────────────────┐  │
│  │ Bootstrap│  │  Session  │  │  PermissionPolicy  │  │
│  │ 引导管理器│  │ 会话引擎 │  │  5 级权限模型      │  │
│  ├──────────┤  ├──────────┤  ├────────────────────┤  │
│  │ HookRunner │  │  Subagent │  │ConversationRuntime │  │
│  │ 双拦截系统 │  │ 子Agent  │  │  核心执行引擎      │  │
│  └──────────┘  └──────────┘  └────────────────────┘  │
├──────────────────────────────────────────────────────┤
│                   LLM API Client                       │
├──────────────────────────────────────────────────────┤
│                   Tool Executor                        │
└──────────────────────────────────────────────────────┘
```

### 核心模块职责

| 模块 | 源文件 | 核心职责 |
|------|--------|---------|
| **ConversationRuntime** | `conversation_core.rs` | 核心执行引擎，驱动 LLM 请求-响应-工具调用的循环，管理 max_iterations 上限与 Token 追踪 |
| **Session** | `session.rs` | 会话状态维护，消息历史管理，基于 JSON 的版本化序列化 |
| **PermissionPolicy** | `permissions.rs` | 5 级权限模型（ReadOnly → Allow），基于 Rust 类型系统的工具访问控制 |
| **HookRunner** | `hook_system.rs` | PreToolUse 与 PostToolUse 双拦截机制，三态退出码（Allow/Deny/Warn） |
| **SubagentToolExecutor** | `agent_tools.rs` | 子 Agent 白名单隔离，四种 Agent 类型的差异化权限分配 |
| **BootstrapPlan** | `bootstrap.rs` | 12 阶段自适应引导系统，FastPath 竞争启动与阶段去重 |
| **ToolSpecs** | `tools/lib.rs` | 19 个 MVP 工具的 schema 定义与输入输出模式 |

---

## 🔬 核心系统深度分析

### 一、权限控制系统 — PermissionPolicy

#### 5 级权限金字塔

权限模型定义了 5 个递增的访问级别，基于 Rust 的 `PartialOrd` 派生实现了编译时排序，无需运行时比较逻辑：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionMode {
    ReadOnly,            // 只读操作（read_file, grep_search...）
    WorkspaceWrite,      // 工作区写操作（write_file, edit...）
    DangerFullAccess,    // 危险操作（bash, powershell...）
    Prompt,              // 需要用户确认
    Allow,               // 完全放行
}
```

排序关系：`ReadOnly < WorkspaceWrite < DangerFullAccess < Prompt < Allow`

#### 关键设计决策

**`Prompt > DangerFullAccess`**：这一排序意味着"需要用户确认"的权限等级高于"纯粹的危险操作"。它的设计逻辑在于——Prompt 模式引入了人与回路（Human-in-the-Loop），用户在确认过程中形成了额外的安全屏障。相比之下，DangerFullAccess 虽然允许执行危险操作，但工具执行本身没有人的参与。

这一决策反映了 ClaudeCode 的安全优先级原则：宁可打断用户的工作流，也要避免未被确认的误操作。

#### 权限判定算法

每个工具注册其所需的最低权限等级。当 LLM 请求调用某个工具时，系统比较当前会话的权限模式与工具的要求：

```rust
// 会话模式 >= 工具要求 → 放行
fn authorize(&self, tool_name: &str, tool_input: &str) -> AuthorizationOutcome {
    let requirement = self.tool_requirements.get(tool_name);
    match requirement {
        Some(level) if self.session_mode >= *level => Outcome::Allowed,
        Some(_) => Outcome::Denied("当前权限级别不足以执行此操作"),
        None => Outcome::Denied("未注册的工具"),
    }
}
```

---

### 二、Hook 拦截系统 — HookRunner

#### 双拦截点设计

Hook 系统在工具执行的前后各设一道关卡，形成完整的执行审计链：

```
  [LLM 请求调用工具]                    [执行结果返回]
         │                                   ▲
         ▼                                   │
    ┌──────────┐    ┌──────────────┐    ┌──────────┐
    │ PreHook  │───▶│ Tool Executor │───▶│ PostHook │
    └──────────┘    └──────────────┘    └──────────┘
         │                                   │
    ┌────▼────┐                         ┌────▼────┐
    │ Allow←0  │                        │ 检测错误 │
    │ Deny ←2  │                        │ 验证输出 │
    │ Warn ←other                       │ 记录日志 │
    └─────────┘                         └─────────┘
```

#### 退出码三态语义

```rust
pub fn run_pre_tool_use(&self, tool_name: &str, tool_input: &str) -> HookRunResult {
    // 返回值语义：
    // 0  = Allow  — 允许执行，继续流程
    // 2  = Deny   — 拒绝执行，阻断工具调用
    // 其他 = Warn  — 允许执行但记录警告
}
```

三态设计比简单的通过/拒绝更精细——Warn 状态允许操作继续，但在返回结果中标记异常，为审计追踪和运营监控留出了空间。

#### Hook 上下文变量

系统在 Hook 执行时注入以下环境变量，供外部脚本判断：

| 变量名 | 出现阶段 | 说明 |
|--------|---------|------|
| `HOOK_EVENT` | Pre/Post | 事件类型（PreToolUse / PostToolUse） |
| `HOOK_TOOL_NAME` | Pre/Post | 被调用的工具名称 |
| `HOOK_TOOL_INPUT` | Pre/Post | 工具的输入参数 |
| `HOOK_TOOL_OUTPUT` | PostOnly | 工具的执行输出 |
| `HOOK_TOOL_IS_ERROR` | PostOnly | 执行是否出错 |

---

### 三、子 Agent 机制 — SubagentToolExecutor

#### 主从架构

ClaudeCode 采用主从（Master-Slave）架构管理子 Agent，这与 AutoGen 的对等架构存在本质区别：

```
主 Agent (ConversationRuntime)
│
├─ spawn_agent_job("Explore", task)
│   → 线程池隔离执行
│   → 白名单：read_file, glob_search, grep_search, WebFetch...
│   → 禁止：bash, write_file 等有副作用的操作
│   → 返回 AgentOutput
│
├─ spawn_agent_job("Plan", task)
│   → 白名单：read_file, glob_search, TodoWrite, WebFetch...
│   → 禁止：bash（可规划但不能执行）
│   → 返回 AgentOutput
│
├─ spawn_agent_job("Verification", task)
│   → 白名单：bash, read_file, WebSearch, ToolSearch...
│   → 允许执行验证所需的所有操作
│   → 返回 AgentOutput
│
└─ spawn_agent_job("Default", task)
    → 全部 19 个工具可用
    → 完整权限
```

#### 白名单隔离机制

```rust
fn allowed_tools_for_subagent(subagent_type: &str) -> BTreeSet<String> {
    match subagent_type {
        "Explore" => vec![
            "read_file", "glob_search", "grep_search",
            "WebFetch", "WebSearch", "ToolSearch",
            "Skill", "StructuredOutput"
        ],
        "Plan" => vec![
            "read_file", "glob_search", "grep_search",
            "WebFetch", "WebSearch", "ToolSearch",
            "TodoWrite", "Skill", "StructuredOutput", "SendUserMessage"
        ],
        "Verification" => vec![
            "bash", "read_file", "glob_search", "grep_search",
            "WebFetch", "WebSearch", "ToolSearch",
            "TodoWrite", "StructuredOutput", "SendUserMessage", "PowerShell"
        ],
        _ => vec![/* 完整 19 个工具 */],
    }
}
```

#### 线程隔离执行

子 Agent 通过 `thread::spawn` 在独立线程中执行，主线程通过 `join()` 等待结果：

```rust
fn execute_subagent(subagent_type: &str, task: &str) -> AgentOutput {
    // 第一步：根据 Agent 类型获取白名单
    let allowed_tools = allowed_tools_for_subagent(subagent_type);

    // 第二步：在线程池中创建隔离的子 Agent
    let handle = thread::spawn(move || {
        let executor = SubagentToolExecutor::new(allowed_tools);
        run_agent_job(task, executor)
    });

    // 第三步：等待子 Agent 完成并返回结果
    handle.join().unwrap()
}
```

---

### 四、Bootstrap 引导系统 — BootstrapPlan

ClaudeCode 的启动流程分为 **12 个阶段**（BootstrapPhase），采用 FastPath 竞争机制——多个路径同时尝试启动，谁先完成谁胜出：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BootstrapPhase {
    CliEntry,                       // CLI 入口初始化
    FastPathVersion,                // 版本快速检测
    StartupProfiler,                // 启动性能分析器
    SystemPromptFastPath,          // 系统提示词组装
    ChromeMcpFastPath,             // Chrome MCP 连接
    DaemonWorkerFastPath,          // 守护进程工作线程
    BridgeFastPath,                // 桥接模式
    DaemonFastPath,                // 守护进程模式
    BackgroundSessionFastPath,     // 后台会话
    TemplateFastPath,              // 模板初始化
    EnvironmentRunnerFastPath,     // 环境运行器
    MainRuntime,                   // 主运行时（最终阶段）
}
```

启动计划支持两种构建方式：

- **`BootstrapPlan::claw_default()`**：生成包含全部 12 阶段的完整启动计划（"claw"在英语中是抓取的意思，这里表示选择全部路径）
- **`BootstrapPlan::from_phases(phases)`**：从自定义阶段列表构建启动计划

两种方式均内置 **阶段去重（deduplication）** 机制，确保同阶段逻辑只执行一次，避免重复初始化。

---

## ⚙️ 执行引擎深度分析

### ConversationRuntime 核心结构

```rust
pub struct ConversationRuntime<C, T> {
    session: Session,                    // 会话状态
    api_client: C,                       // LLM API 客户端（泛型）
    tool_executor: T,                    // 工具执行器（泛型）
    permission_policy: PermissionPolicy, // 权限策略
    system_prompt: Vec<String>,          // 系统提示词
    max_iterations: usize,               // 最大迭代次数（安全上限）
    usage_tracker: UsageTracker,         // Token 用量追踪
    hook_runner: HookRunner,             // Hook 运行器
}
```

其中 `C` 和 `T` 是泛型参数，分别代表 LLM API Client 和 Tool Executor 的类型——这一设计允许在不同环境下注入不同的实现，体现了 Rust 泛型与 trait 的组合能力。

### 核心循环：run_turn()

`run_turn()` 是 Agent Orchestrator 最核心的执行逻辑，每次调用完成一个完整的用户交互回合：

```
┌─────────────────────────────────────────────────────────┐
│                 run_turn() 执行流程                       │
├─────────────────────────────────────────────────────────┤
│  Step 1: Session::add_user_message()                     │
│          ← 接收用户输入并记录到会话                        │
├─────────────────────────────────────────────────────────┤
│  Step 2: api_client.complete()                           │
│          ← 调用 LLM API，获取响应                          │
├─────────────────────────────────────────────────────────┤
│  Step 3: 主循环（上限 max_iterations）                    │
│                                                          │
│  3.1 解析 LLM 响应，提取 tool_calls                       │
│                                                          │
│  3.2 逐工具执行（安全链路）：                              │
│      ┌─ PermissionPolicy::authorize()    ← 关卡 1         │
│      ├─ HookRunner::run_pre_tool_use()   ← 关卡 2         │
│      ├─ tool_executor.execute()          ← 执行工具        │
│      ├─ HookRunner::run_post_tool_use()  ← 关卡 3         │
│      └─ Session::add_tool_result()       ← 记录结果        │
│                                                          │
│  3.3 判断终止条件：                                       │
│      - 无更多工具调用 → break                             │
│      - 达到 max_iterations → 强制终止                     │
│      - 否则 → 继续下一轮 api_client.complete()             │
├─────────────────────────────────────────────────────────┤
│  Step 4: 返回 TurnSummary                                 │
│          ← 包含本轮所有消息、Token 用量和迭代次数          │
└─────────────────────────────────────────────────────────┘
```

三层安全关卡的逻辑构成了 ClaudeCode 安全架构的基石：**权限防越界、Hook 防恶意、Post-Hook 验结果**。

### Agent 运行时状态机

Agent 在其生命周期中经历四个状态，由 ConversationRuntime 统一管理：

```
             ┌──────────────────────────────────┐
             │          Initial                  │
             │     Launcher 创建 Agent           │
             └────────────┬─────────────────────┘
                          │
                          ▼
             ┌──────────────────────────────────┐
             │          Waiting                  │
   ┌────────▶│    等待用户输入 / LLM 响应         │
   │         └────────────┬─────────────────────┘
   │                      │
   │       ┌──────────────┘
   │       ▼
   │  ┌────────────────────────────────────────┐
   │  │            Executing                   │
   │  │                                        │
   │  │  ┌──────────────┐  ┌───────────────┐  │
   │  │  │  LLM 请求处理 │  │  工具执行循环  │  │
   │  │  │  tool_calls   │  │  Permission   │  │
   │  │  │  解析        │  │  Hook 检查    │  │
   │  │  └──────────────┘  └───────────────┘  │
   │  └────────────────────────────────────────┘
   │                      │
   │        ┌─────────────┘
   │        │      (还有工具调用)
   └────────┘
                          │
                          ▼ (max_iterations 或 无工具调用)
             ┌──────────────────────────────────┐
             │         Terminated                │
             │    Agent 执行完毕，资源释放        │
             └──────────────────────────────────┘
```

### 完整执行伪代码

```rust
pub fn run_turn(&mut self) -> TurnSummary {
    // Step 1: 记录用户输入
    self.session.add_user_message();

    // Step 2: 首次 LLM 请求
    let mut response = self.api_client.complete(&self.session.messages);

    // Step 3: 工具调用循环（上限保护）
    for iteration in 0..self.max_iterations {
        let events = self.parse_events(response);

        if events.tool_calls.is_empty() {
            break; // 无工具调用 → 本轮结束
        }

        for tool_call in &events.tool_calls {
            // 关卡一：权限检查
            if self.permission_policy.authorize(&tool_call).is_deny() {
                self.session.add_tool_result("权限不足");
                continue;
            }

            // 关卡二：Pre-Hook
            let hook = self.hook_runner.run_pre_tool_use(&tool_call);
            if hook.is_deny() {
                self.session.add_tool_result("Hook 拦截");
                continue;
            }

            // 执行工具
            let result = self.tool_executor.execute(&tool_call);

            // 关卡三：Post-Hook
            self.hook_runner.run_post_tool_use(&tool_call, &result);

            // 记录结果
            self.session.add_tool_result(result);
        }

        // 请求 LLM 继续处理工具执行结果
        response = self.api_client.complete(&self.session.messages);
    }

    TurnSummary {
        messages: self.session.messages.clone(),
        usage: self.usage_tracker.current(),
        iterations,
    }
}
```

---

## 🧪 实验验证

### 测试批次与结果

全部 63 个测试用例在四个模块方向上一遍通过，覆盖核心功能、边界条件和异常场景：

```
running 63 tests
test result: ok. 63 passed; 0 failed; 0 ignored; 0 measured
```

| 测试模块 | 用例数 | 通过率 | 验证重点 |
|---------|:------:|:------:|---------|
| Session 会话管理 | 9 | 100% | 消息添加、JSON 序列化与反序列化、版本兼容性 |
| Hook 拦截机制 | 10 | 100% | PreToolUse 阻断危险命令、PostToolUse 错误检测 |
| 权限分级控制 | 11 | 100% | 5 级别排序、ReadOnly 阻断 bash、Prompt > Danger 语义 |
| 子 Agent 权限隔离 | 12 | 100% | Explore/Plan/Verification 的差异化白名单验证 |
| Bootstrap 阶段 | 7 | 100% | 12 阶段完整性、去重机制、from_phases 构建 |
| 工具规格 | 7 | 100% | 19 个工具模式验证 |
| 集成测试 | 5 | 100% | 跨模块端到端流程 |
| **总计** | **63** | **100%** | |

### 关键测试示例

**子 Agent 白名单隔离**：
```rust
#[test]
fn test_explore_agent_blocks_bash() {
    let tools = allowed_tools_for_subagent("Explore");
    assert!(!tools.contains("bash"));
    // Explore Agent 的权限仅限于读取文件与搜索，无权执行系统命令
}

#[test]
fn test_verification_agent_allows_bash() {
    let tools = allowed_tools_for_subagent("Verification");
    assert!(tools.contains("bash"));
    // Verification Agent 需要运行代码来验证结果，故允许 bash
}
```

**Hook 双拦截验证**：
```rust
#[test]
fn test_hook_runner_blocks_dangerous_commands() {
    let config = RuntimeHookConfig::new(
        vec!["rm -rf blocked".to_string()],
        Vec::new(),
    );
    let runner = HookRunner::new(config);
    let result = runner.run_pre_tool_use("bash", r#"{"command":"rm -rf /"}"#);
    assert!(result.is_denied()); // Pre-Hook 捕获并拦截危险命令
}

#[test]
fn test_hook_runner_post_tool_error_detection() {
    let result = runner.run_post_tool_use("bash", input, "ERROR: permission denied", true);
    assert!(result.is_warned()); // Post-Hook 检测到执行错误并发出警告
}
```

**5 级权限排序**：
```rust
#[test]
fn test_permission_mode_ordering() {
    assert!(PermissionMode::ReadOnly < PermissionMode::WorkspaceWrite);
    assert!(PermissionMode::WorkspaceWrite < PermissionMode::DangerFullAccess);
    assert!(PermissionMode::DangerFullAccess < PermissionMode::Prompt);
    assert!(PermissionMode::Prompt < PermissionMode::Allow);
    // 验证 Prompt > DangerFullAccess 这一关键设计决策
}
```

**Bootstrap 阶段去重**：
```rust
#[test]
fn test_bootstrap_plan_deduplication() {
    let plan = BootstrapPlan::from_phases(vec![
        BootstrapPhase::MainRuntime,
        BootstrapPhase::MainRuntime, // 重复阶段
        BootstrapPhase::CliEntry,
    ]);
    assert_eq!(plan.phases().len(), 2); // 去重后仅保留 ManRuntime + CliEntry
}
```

### 运行测试

```bash
cd source_code
rustc --test test_suite.rs -o test_suite
./test_suite
# 输出: running 63 tests, test result: ok. 63 passed; 0 failed
```

---

## 🔄 框架对比

### ClaudeCode vs AutoGen vs MetaGPT

| 对比维度 | ClaudeCode | AutoGen（Microsoft） | MetaGPT |
|---------|:----------:|:--------------------:|:-------:|
| **架构模式** | 主从（Master-Slave） | 对等（Peer-to-Peer） | SOP 流水线 |
| **Agent 协作** | 线程池隔离 + 白名单 | 消息队列 + 灵活路由 | 函数注册 + 标准流程 |
| **权限模型** | 5 级 + Hook 双层 | 无内置 | 无内置 |
| **会话管理** | 版本化 JSON | 基础序列化 | 基础序列化 |
| **启动引导** | 12 阶段 FastPath | N/A | N/A |
| **工具定义** | 白名单 19 个 MVP | 灵活注册 | 函数注册 |
| **安全架构** | 权限 + Hook + 白名单 | 依赖外部 | 依赖外部 |
| **测试体系** | 63 例 100% 通过 | 社区维护 | 社区维护 |

### 本质差异

- **Agent 协作范式**：AutoGen 采用对等架构，所有 Agent 地位平等、直接对话，灵活性高但安全边界模糊；ClaudeCode 采用主从架构，子 Agent 隔离执行、单向通信，安全性更高的代价是灵活性受限。
- **权限控制**：ClaudeCode 是三者中唯一拥有完整权限模型 + Hook 拦截 + 白名单隔离的系统化安全方案的。
- **设计哲学**：ClaudeCode 基于 Rust 的系统编程语言构建，在编译层面利用类型系统保证权限安全；AutoGen 和 MetaGPT 基于 Python，更注重快速迭代和灵活性。

---

## 💡 研究发现与洞见

### 架构哲学

1. **Rust 类型系统用于权限建模**：利用 `#[derive(PartialOrd, Ord)]`，ClaudeCode 在编译时而非运行时完成权限等级的排序与比较。这体现了 Rust 的"零成本抽象"——安全不需要付出运行时性能代价。

2. **Agent 的安全困境**：Agent 的能力与安全之间存在着根本性矛盾——Agent 越强大（能做的事越多），越需要严格限制；但限制越多，Agent 的自主性越弱，其作为"智能体"的价值就越低。这种张力贯穿了整个 Agent Orchestrator 的设计，本质上是 **能力** 与 **信任** 之间的权衡。

3. **安全靠架构而非提示词**：ClaudeCode 没有依赖 prompt engineering 来"劝"LLM 不要做危险操作，而是通过权限系统、Hook 拦截、子 Agent 白名单三层架构来**硬性保证**安全边界。这个选择反映了 ClaudeCode 的设计判断：*随着 AI 能力的提升，对安全不可靠的人为约束将越来越不可靠。*

### 改进建议

| 当前局限 | 建议方案 |
|---------|---------|
| Hook 配置固化在代码中 | 改为外部配置文件 + 热加载机制 |
| 权限仅检查工具级别 | 增加参数级别的细粒度校验（如：允许 bash 但禁止 rm） |
| Token 追踪无上限中断 | 引入阶梯式 Token 预算告警 |
| 子 Agent 单向通信 | 引入 Agent 结果总线，允许跨 Agent 共享中间结果 |
| 安全策略不可插拔 | 抽象为 Policy Provider trait，支持自定义安全策略 |

### 未来展望

本研究的核心结论是：**Agentic System 的未来不在于让单个 Agent 更强，而在于让多个各有所长的 Agent 在清晰的安全边界内更有效地协作。** ClaudeCode 的 Agent Orchestrator 展示了一条值得借鉴的路径——用严格的架构约束换取更可靠的行为边界，而非用更复杂的提示词来"祈祷"LLM 不要犯错。

---

## 📚 参考资料

- [ClaudeCode 官方文档](https://docs.anthropic.com/en/docs/claude-code)
- [AutoGen (Microsoft)](https://microsoft.github.io/autogen/)
- [MetaGPT (DeepWisdom)](https://github.com/DeepWisdom/MetaGPT)
- [OWASP Agentic System Security](https://owasp.org/www-project-ai-security/)

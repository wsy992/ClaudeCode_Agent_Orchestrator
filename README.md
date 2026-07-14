# ClaudeCode Agent Orchestrator — 源码级架构研究

<p align="center">
  <b>对 ClaudeCode 多 Agent 调度核心的深度源码分析</b><br>
  7 个核心模块 · 63 个测试用例 · 4 万字研究报告 · 与 AutoGen / MetaGPT 多维对比
</p>

---

## 📖 研究背景

2024 年以来，以 ClaudeCode 为代表的 AI Agent 编程工具正在改变软件开发的方式——它们不再只是"代码补全"，而是能自主理解需求、规划步骤、执行命令并处理错误的智能体。

但一个核心问题始终悬而未决：**当 AI Agent 拥有 bash 权限、可以读写文件、能联网搜索时，如何保证它不会越界？**

本项目选择 ClaudeCode 的 **Agent Orchestrator（多 Agent 调度器）** 作为切入点，通过源码级分析，回答三个核心问题：

1. ClaudeCode 的"Agent"究竟是如何运作的？它和单纯的 LLM API 调用有什么区别？
2. 多个 Agent 之间如何协调？复杂任务如何拆解为 Explore → Plan → Verify 的协作流水线？
3. AI 拥有 shell 权限时，安全边界如何设计？权限模型和 Hook 拦截如何层层设防？

---

## 🏗️ 核心架构

### 分层设计

Agent Orchestrator 在 ClaudeCode 整体架构中处于中枢位置，连接 CLI/GUI、LLM API、工具执行器和会话管理层：

```
┌─────────────────────────────────────────────────┐
│                  CLI / GUI 入口                    │
├─────────────────────────────────────────────────┤
│             Agent Orchestrator (核心调度器)        │
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │
│  │ Bootstrap│  │  Session  │  │  Permission  │  │
│  │ 引导阶段  │  │ 会话管理  │  │  权限策略     │  │
│  ├──────────┤  ├──────────┤  ├──────────────┤  │
│  │   Hook   │  │   Agent  │  │Conversation- │  │
│  │ 拦截系统  │  │ 子Agent  │  │   Runtime    │  │
│  └──────────┘  └──────────┘  └──────────────┘  │
├─────────────────────────────────────────────────┤
│                  LLM API Client                    │
├─────────────────────────────────────────────────┤
│                  Tool Executor                    │
└─────────────────────────────────────────────────┘
```

### 核心模块一览

| 模块 | 源文件 | 职责 |
|------|--------|------|
| **ConversationRuntime** | `conversation_core.rs` | 核心执行引擎，负责 LLM 请求-响应循环、工具调用编排、Token 追踪 |
| **Session** | `session.rs` | 会话管理，维护消息历史、版本化 JSON 序列化 |
| **PermissionPolicy** | `permissions.rs` | 5 级权限模型，控制工具调用的访问级别 |
| **HookRunner** | `hook_system.rs` | Pre/Post 双拦截钩子，工具执行前后安全检查 |
| **SubagentToolExecutor** | `agent_tools.rs` | 子 Agent 白名单隔离，按类型差异化权限分配 |
| **BootstrapPlan** | `bootstrap.rs` | 12 阶段引导系统，优化启动路径 |
| **ToolSpecs** | `tools/lib.rs` | 19 个 MVP 工具定义与模式 |

---

## 🔬 四大核心系统深度分析

### 1. 权限控制系统 — 5 级金字塔

权限模型从宽到严分为 5 个等级，基于 Rust 的 `PartialOrd` 派生实现编译时排序：

```
Allow（完全放行）
│
Prompt（需要用户确认）
│
DangerFullAccess（危险操作 — bash）
│
WorkspaceWrite（工作区写操作）
│
ReadOnly（只读操作）
```

**关键设计决策**：`Prompt > DangerFullAccess`，即需要用户确认的操作权限等级高于纯粹的"危险操作"——因为 Prompt 引入了人在回路中（Human-in-the-Loop）的额外安全屏障。这一设计反映了 ClaudeCode 的安全优先级：**宁可打断用户，也要避免误操作**。

### 2. Hook 拦截系统 — 双检哨兵

```
  [LLM 请求调用工具]                    [执行结果返回]
         │                                   ▲
         ▼                                   │
    ┌─────────┐    ┌──────────────┐    ┌──────────┐
    │ PreHook │───▶│ Tool Executor │───▶│ PostHook │
    └─────────┘    └──────────────┘    └──────────┘
         │                                   │
    ┌────▼────┐                         ┌────▼────┐
    │ Allow←0  │                        │ 检测错误 │
    │ Deny ←2  │                        │ 验证输出 │
    │ Warn ←other                       │ 记录日志 │
    └─────────┘                         └─────────┘
```

退出码三态设计（Allow / Deny / Warn）比简单的通过/拒绝更精细——Warn 状态允许执行但标记结果，为审计和监控留出了空间。

### 3. 子 Agent 机制 — 主从架构 + 白名单隔离

ClaudeCode 的 Agent 协作采用**主从（Master-Slave）** 架构，与 AutoGen 的对等架构有本质区别：

```
主 Agent (ConversationRuntime)
│
├─ Explore Agent：仅读文件、搜索代码，无权执行 bash
├─ Plan Agent：读文件 + 写入任务列表，无权执行 bash
├─ Verification Agent：可执行 bash，用于验证结果
└─ Default Agent：全部 19 个工具
```

每个子 Agent 运行在独立的线程池中，通过 `allowed_tools_for_subagent()` 函数进行白名单限制：

```rust
fn allowed_tools_for_subagent(agent_type: &str) -> BTreeSet<String> {
    match agent_type {
        "Explore" => vec!["read_file", "glob_search", "grep_search", ...],
        "Plan"    => vec!["read_file", "TodoWrite", "WebFetch", ...],
        "Verification" => vec!["bash", "read_file", "WebSearch", ...],
        _         => vec![/* 全部 19 个工具 */],
    }
}
```

### 4. Bootstrap 引导系统 — 12 阶段自适应启动

ClaudeCode 的启动流程分为 12 个阶段，采用 FastPath 竞争机制——多个路径同时尝试，谁先完成谁胜出：

```
CLI Entry → FastPathVersion → StartupProfiler → SystemPromptFastPath
→ ChromeMcpFastPath → DaemonWorkerFastPath → BridgeFastPath
→ DaemonFastPath → BackgroundSessionFastPath → TemplateFastPath
→ EnvironmentRunnerFastPath → MainRuntime
```

**去重机制**：BootstrapPlan 实现了阶段去重（deduplication），当多个 FastPath 竞争启动时，同阶段逻辑只执行一次，避免重复初始化。

---

## ⚙️ 执行引擎深度分析

### 主循环：ConversationRuntime::run_turn()

这是 Agent Orchestrator 最核心的执行逻辑，完整的工具调用安全链路包含三层关卡：

```
1. Session::add_user_message()    ← 接收用户输入
2. api_client.complete()          ← 调用 LLM API
3. loop (max_iterations 上限保护):
   a. 解析 LLM 响应中的 tool_calls
   b. 逐工具调用：
      - 关卡 1：PermissionPolicy::authorize()      权限检查
      - 关卡 2：HookRunner::run_pre_tool_use()     Pre-Hook
      - 关卡 3：tool_executor.execute()             执行工具
      - 关卡 4：HookRunner::run_post_tool_use()    Post-Hook
      - Session::add_tool_result()                  记录结果
   c. 若无更多工具调用 → break
   d. api_client.complete()  ← 下一轮 LLM 请求
4. 返回 TurnSummary (含 Token 用量)
```

### Agent 运行时状态机

Agent 在四个状态间转换，由 ConversationRuntime 管理生命周期：

```
Initial → Waiting → Executing → Waiting → ... → Terminated
               │          │
               │     ┌────┴────┐
               │     │ LLM API │
               │     │ 工具执行 │
               │     │ Hook检查 │
               │     └─────────┘
               │
           ┌───┴───┐
           │ 接收   │
           │ 用户   │
           │ 输入   │
           └───────┘
```

---

## 🧪 实验验证

### 测试结果

```
running 63 tests
test result: ok. 63 passed; 0 failed; 0 ignored; 0 measured
```

| 模块 | 测试数 | 通过率 | 核心验证点 |
|------|:------:|:------:|-----------|
| Session 会话管理 | 9 | 100% | 消息序列化/反序列化、版本兼容性 |
| Hook 拦截机制 | 10 | 100% | PreToolUse 阻断危险命令、PostToolUse 错误检测 |
| 权限分级控制 | 11 | 100% | ReadOnly 阻断 bash、Prompt > DangerFullAccess 排序 |
| 子 Agent 权限隔离 | 12 | 100% | Explore 无权运行 bash、Verification 有权 |
| Bootstrap 阶段 | 7 | 100% | 12 阶段完整性、去重机制 |
| 工具规格 | 7 | 100% | 19 个 MVP 工具模式验证 |
| 集成测试 | 5 | 100% | 跨模块端到端流程 |
| **总计** | **63** | **100%** | |

### 关键测试案例

```rust
// Explore Agent 尝试执行 bash → 应被拒绝
#[test]
fn test_explore_agent_blocks_bash() {
    let tools = allowed_tools_for_subagent("Explore");
    assert!(!tools.contains("bash")); // bash 不在白名单中
}

// 5 级权限排序验证
#[test]
fn test_permission_mode_ordering() {
    assert!(ReadOnly < WorkspaceWrite < DangerFullAccess);
    assert!(DangerFullAccess < Prompt < Allow); // Prompt > Danger!
}
```

---

## 🔄 框架对比

| 特性维度 | ClaudeCode | AutoGen (Microsoft) | MetaGPT |
|---------|:----------:|:-------------------:|:-------:|
| **协作模式** | 主从（Master-Slave） | 对等（Peer-to-Peer） | SOP 流水线 |
| **权限模型** | 5 级 + Hook 双层 | 无内置 | 无内置 |
| **Agent 隔离** | 线程池 + 白名单 | 消息队列 | 函数注册 |
| **会话持久化** | 版本化 JSON | 基础序列化 | 基础序列化 |
| **Bootstrap** | 12 阶段自适应 | N/A | N/A |
| **Hook 机制** | Pre/Post 双拦截 | 有限 | 无 |
| **工具定义** | 19 个 MVP 白名单 | 灵活注册 | 函数注册 |
| **测试体系** | 63 例全通过 | 社区维护 | 社区维护 |

**核心差异**：
- ClaudeCode 是三者中唯一拥有完整安全架构的（权限 + Hook + 白名单）
- AutoGen 在 Agent 间通信灵活性上更优，但对等架构带来安全边界模糊问题
- MetaGPT 以 SOP 驱动，适合有明确步骤的任务，但在动态决策上受限

---

## 💡 研究发现与洞见

### 架构哲学

1. **Rust 类型系统用于权限建模**：利用 `#[derive(PartialOrd, Ord)]` 实现权限等级的编译时排序，无需运行时比较逻辑，体现了 Rust 的"零成本抽象"理念。

2. **Agent 的安全困境**：Agent 越强大，越需要严格限制；限制越多，Agent 的自主性越弱。这种张力贯穿了整个 Orchestrator 设计——它本质上是在**能力**和**安全**之间的权衡。

3. **安全靠架构不靠提示词**：ClaudeCode 没有依赖提示词来约束 LLM 行为，而是通过权限系统、Hook 拦截、子 Agent 隔离三层架构来硬性保证安全边界。

### 改进建议

| 当前局限 | 优化方向 |
|---------|---------|
| Hook 写死在代码中 | 外部配置 + 热加载规则 |
| 权限仅检查工具级别 | 增加参数级别的细粒度校验 |
| Token 追踪无上限断点 | 阶梯式 Token 预算告警 |
| 子 Agent 单向通信 | 引入 Agent 结果总线 |

---

## 📁 项目结构

```
├── README.md                                               # 本文件
├── ClaudeCode_Agent_Orchestrator_研究报告_正式版.md        # 完整研究报告（4 万字）
├── ClaudeCode.pptx                                         # 汇报 PPT
│
├── claudecode-main/                                        # ClaudeCode 原始源码
│   └── rust/crates/runtime/src/
│       ├── bootstrap.rs          (引导阶段管理)
│       ├── permissions.rs        (权限控制系统)
│       ├── hooks.rs              (Hook 拦截系统)
│       ├── session.rs            (会话管理)
│       └── conversation.rs       (对话引擎)
│
└── source_code/                                            # 研究用源码 & 测试
    ├── conversation_core.rs                                # ConversationRuntime 核心
    ├── agent_tools.rs                                      # 子 Agent 工具执行器
    ├── permissions.rs                                      # 权限系统复现
    ├── hook_system.rs                                      # Hook 系统复现
    ├── bootstrap.rs                                        # Bootstrap 系统复现
    ├── session.rs                                          # 会话管理复现
    ├── experiments.rs                                      # 扩展实验
    ├── test_suite.rs                                       # 63 个测试用例
    └── README.md                                           # 源码索引
```

---

## ▶️ 运行测试

```bash
cd source_code
rustc --test test_suite.rs -o test_suite
./test_suite
# 输出: running 63 tests, test result: ok. 63 passed; 0 failed
```

---

## 📚 参考资料

- [ClaudeCode 官方文档](https://docs.anthropic.com/en/docs/claude-code)
- [AutoGen (Microsoft)](https://microsoft.github.io/autogen/)
- [MetaGPT (DeepWisdom)](https://github.com/DeepWisdom/MetaGPT)
- [OWASP Agentic System Security](https://owasp.org/www-project-ai-security/)

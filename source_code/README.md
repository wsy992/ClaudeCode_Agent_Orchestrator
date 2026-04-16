# ClaudeCode Agent Orchestrator 源代码文件索引

本目录包含 ClaudeCode 项目 Agent Orchestrator 模块的完整源代码及详细注释。

## 文件列表

### 1. session.rs
**会话与消息结构**

核心数据结构定义：
- `Session`: 对话会话，包含版本号和消息历史
- `ConversationMessage`: 对话消息，包含角色和内容块
- `ContentBlock`: 消息内容块（Text/ToolUse/ToolResult）
- `MessageRole`: 消息角色（System/User/Assistant/Tool）
- `TokenUsage`: Token 使用量统计

关键功能：
- 会话持久化（save_to_path/load_from_path）
- JSON 序列化/反序列化
- 消息构建辅助函数

---

### 2. conversation_core.rs
**Agent 核心执行引擎**

`ConversationRuntime<C, T>` - Agent 的核心执行引擎：
- 泛型设计：支持多种 API 客户端和工具执行器
- `run_turn()` - Agent 执行一圈的核心方法
- 工具调用循环处理
- 权限检查和 Hook 拦截

关键流程：
```
1. 添加用户消息到会话
2. 循环直到没有待执行工具:
   a. 调用 LLM API 获取响应
   b. 解析响应事件流
   c. 提取待执行的工具调用
   d. 对每个工具:
       i.   检查权限
       ii.  执行 PreToolUse Hook
       iii. 执行工具
       iv.  执行 PostToolUse Hook
       v.   将结果添加回会话
3. 返回 TurnSummary
```

辅助类型：
- `ApiRequest` / `ApiClient` trait
- `TurnSummary` - 执行结果摘要
- `StaticToolExecutor` - 静态工具执行器

---

### 3. agent_tools.rs
**子Agent与工具执行实现**

子Agent机制：
- `AgentInput` / `AgentOutput` - Agent 创建和输出结构
- `AgentJob` - 在线程中执行的Agent任务
- `execute_agent()` - 启动子Agent的主入口
- `spawn_agent_job()` - 在独立线程中启动
- `run_agent_job()` - 执行单个Agent任务

工具权限隔离：
- `allowed_tools_for_subagent()` - 根据Agent类型确定允许的工具集
- `SubagentToolExecutor` - 带白名单过滤的工具执行器

子Agent类型：
- `Explore`: 只读工具（代码探索）
- `Plan`: 只读 + 任务写入
- `Verification`: 包含bash执行
- `claw-guide`: 指南工具
- `statusline-setup`: 状态栏设置
- `default`: 完整权限

MVP 工具列表（mvp_tool_specs）：
- 系统工具: bash, PowerShell
- 文件工具: read_file, write_file, edit_file
- 搜索工具: glob_search, grep_search
- Web工具: WebFetch, WebSearch
- 任务工具: TodoWrite, Skill
- 通信工具: SendUserMessage
- Agent工具: Agent

---

### 4. hook_system.rs
**Hook 拦截机制**

`HookRunner` - 工具执行生命周期拦截：
- `run_pre_tool_use()` - 工具执行前拦截
- `run_post_tool_use()` - 工具执行后拦截

Hook 类型：
- `HookEvent`: PreToolUse / PostToolUse
- `HookRunResult`: 包含 denied 标志和消息列表

退出码语义：
- 0 = Allow - 允许执行
- 2 = Deny - 拒绝执行
- 其他 = Warn - 警告但继续

环境变量（传递给 Hook 脚本）：
- `HOOK_EVENT`: 事件类型
- `HOOK_TOOL_NAME`: 工具名称
- `HOOK_TOOL_INPUT`: 工具输入
- `HOOK_TOOL_OUTPUT`: 工具输出（PostToolUse）
- `HOOK_TOOL_IS_ERROR`: 是否为错误

使用示例：
```rust
// 阻止危险命令
let config = RuntimeHookConfig::new(
    vec![r#"if echo "$HOOK_TOOL_INPUT" | grep -q "rm -rf"; then exit 2; fi"#.to_string()],
    Vec::new(),
);
```

---

### 5. bootstrap.rs
**引导阶段管理**

`BootstrapPhase` - 引导阶段枚举：
- `CliEntry`: CLI 入口点
- `FastPathVersion`: 快速版本检查
- `StartupProfiler`: 启动性能分析
- `SystemPromptFastPath`: 系统提示快速路径
- `ChromeMcpFastPath`: Chrome MCP 快速路径
- `DaemonWorkerFastPath`: 守护进程工作线程
- `BridgeFastPath`: 桥接快速路径
- `DaemonFastPath`: 守护进程快速路径
- `BackgroundSessionFastPath`: 后台会话快速路径
- `TemplateFastPath`: 模板快速路径
- `EnvironmentRunnerFastPath`: 环境运行器快速路径
- `MainRuntime`: 主运行时

`BootstrapPlan` - 引导计划：
- `claw_default()` - 创建默认引导计划
- `from_phases()` - 从阶段列表创建（自动去重）
- `phases()` - 获取阶段列表

设计特点：
- 阶段化启动
- 去重机制
- 可组合的差异化启动

---

### 6. permissions.rs
**权限控制系统**

`PermissionMode` - 权限级别（从低到高）：
- `ReadOnly`: 只读
- `WorkspaceWrite`: 工作区写
- `DangerFullAccess`: 危险的全权限
- `Prompt`: 需要用户确认
- `Allow`: 允许所有

`PermissionPolicy` - 权限策略：
- 管理当前权限模式
- 定义特定工具的权限要求
- `authorize()` - 授权决策

`PermissionPrompter` trait - 权限提示接口：
- `decide()` - 向用户发出提示并获取决策

授权算法：
1. 如果当前模式为 Allow，或当前模式 >= 所需模式，允许
2. 如果当前模式为 Prompt 或需要升级到 DangerFullAccess，提示用户
3. 否则拒绝

使用示例：
```rust
let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
    .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

let outcome = policy.authorize("bash", "{}", None);
// -> Deny { reason: "tool 'bash' requires danger-full-access" }
```

---

### 7. experiments.rs
**实验验证代码**

包含 7 个实验模块：

#### 7.1 subagent_permission_isolation_tests
验证子Agent工具权限隔离：
- Explore Agent 无法执行 bash
- Verification Agent 可以执行 bash
- 权限边界严格强制执行

#### 7.2 hook_interception_tests
验证 Hook 拦截机制：
- PreToolUse Hook 可以阻止执行
- PostToolUse Hook 可以修改输出
- 多个 Hook 按顺序执行

#### 7.3 agent_collaboration_tests
验证多Agent协作：
- Agent 工具能够创建子Agent任务
- 子Agent在独立线程中执行
- 执行结果正确传递

#### 7.4 permission_policy_tests
验证权限策略分级控制：
- ReadOnly 模式阻止写操作
- WorkspaceWrite 允许读写但阻止危险操作
- DangerFullAccess 允许所有操作

#### 7.5 session_persistence_tests
验证会话持久化：
- 会话消息正确序列化
- 工具调用和结果正确记录
- 恢复后状态一致

#### 7.6 bootstrap_tests
验证引导阶段执行：
- 阶段按顺序执行
- 去重机制正常工作
- 可以跳过某些阶段

#### 7.7 concurrency_tests
验证并发安全：
- 多个 Agent 同时更新会话
- 工具执行计数准确性
- 无数据竞争

---

## 编译和运行

```bash
# 编译实验代码
rustc --test experiments.rs -o experiments

# 运行所有实验
./experiments

# 运行特定实验
./experiments --test subagent_permission_isolation_tests
```

---

## 依赖关系

```
session.rs (基础数据结构)
    ↓
conversation_core.rs (核心执行引擎)
    ↓
agent_tools.rs (子Agent机制) + hook_system.rs (Hook拦截) + permissions.rs (权限控制)
    ↓
bootstrap.rs (引导阶段管理)
    ↓
experiments.rs (实验验证)
```

---

*最后更新: 2026-04-14*

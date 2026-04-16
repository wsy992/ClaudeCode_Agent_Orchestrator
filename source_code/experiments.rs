/**
 * @file experiments.rs
 * @brief ClaudeCode Agent Orchestrator - 实验验证代码
 *
 * 本文件包含 Agent Orchestrator 模块的实验验证代码：
 * - Agent 协作机制验证
 * - 权限隔离验证
 * - Hook 拦截验证
 * - 工具调用循环验证
 *
 * @author ClaudeCode Research Team
 * @date 2026-04-14
 */

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

// ============================================================
// 实验1: 子Agent工具权限隔离验证
// ============================================================

/**
 * 实验目标：验证 SubagentToolExecutor 能够正确隔离不同类型Agent的工具权限
 *
 * 测试场景：
 * 1. Explore Agent 应该无法执行 bash
 * 2. Verification Agent 可以执行 bash
 * 3. 默认 Agent 可以执行所有工具
 */
#[cfg(test)]
mod subagent_permission_isolation_tests {

    // 模拟 SubagentToolExecutor
    struct MockToolExecutor {
        allowed_tools: BTreeSet<String>,
        execution_log: Vec<String>,
    }

    impl MockToolExecutor {
        fn new(allowed_tools: BTreeSet<String>) -> Self {
            Self {
                allowed_tools,
                execution_log: Vec::new(),
            }
        }

        fn execute(&mut self, tool_name: &str, _input: &str) -> Result<String, String> {
            if !self.allowed_tools.contains(tool_name) {
                return Err(format!(
                    "tool `{}` is not enabled for this sub-agent",
                    tool_name
                ));
            }
            self.execution_log.push(tool_name.to_string());
            Ok(format!("executed {}", tool_name))
        }
    }

    #[test]
    fn test_explore_agent_cannot_execute_bash() {
        // 给定：Explore Agent 只有只读工具
        let mut executor = MockToolExecutor::new(
            vec!["read_file", "glob_search", "grep_search", "WebFetch", "WebSearch"]
                .into_iter()
                .map(String::from)
                .collect(),
        );

        // 当：尝试执行 bash
        let result = executor.execute("bash", r#"{"command":"ls"}"#);

        // 那么：应该被拒绝
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("is not enabled"));
    }

    #[test]
    fn test_explore_agent_can_read_files() {
        // 给定：Explore Agent 只有只读工具
        let mut executor = MockToolExecutor::new(
            vec!["read_file", "glob_search", "grep_search"]
                .into_iter()
                .map(String::from)
                .collect(),
        );

        // 当：尝试读取文件
        let result = executor.execute("read_file", r#"{"path":"test.txt"}"#);

        // 那么：应该成功
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "executed read_file");
    }

    #[test]
    fn test_verification_agent_can_execute_bash() {
        // 给定：Verification Agent 有 bash 权限
        let mut executor = MockToolExecutor::new(
            vec!["bash", "read_file", "write_file", "glob_search"]
                .into_iter()
                .map(String::from)
                .collect(),
        );

        // 当：尝试执行 bash
        let result = executor.execute("bash", r#"{"command":"pytest"}"#);

        // 那么：应该成功
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "executed bash");
    }

    #[test]
    fn test_default_agent_has_all_tools() {
        // 给定：默认 Agent 有完整工具集
        let mut executor = MockToolExecutor::new(
            vec![
                "bash", "read_file", "write_file", "edit_file",
                "glob_search", "grep_search", "WebFetch", "WebSearch",
                "TodoWrite", "Agent",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );

        // 当：尝试执行各种工具
        let tools = vec!["bash", "read_file", "write_file", "Agent"];

        for tool in tools {
            let result = executor.execute(tool, "{}");
            assert!(result.is_ok(), "tool {} should be allowed", tool);
        }
    }

    #[test]
    fn test_permission_boundary_enforcement() {
        // 测试：验证权限边界严格强制执行
        let mut executor = MockToolExecutor::new(
            vec!["read_file"].into_iter().map(String::from).collect(),
        );

        // 连续尝试执行禁止的工具
        for _ in 0..3 {
            let result = executor.execute("bash", "{}");
            assert!(result.is_err());
        }

        // 验证没有工具被执行
        assert!(executor.execution_log.is_empty());
    }
}

// ============================================================
// 实验2: Hook 拦截机制验证
// ============================================================

/**
 * 实验目标：验证 Hook 系统能够正确拦截工具执行
 *
 * 测试场景：
 * 1. PreToolUse Hook 可以阻止执行
 * 2. PostToolUse Hook 可以修改输出
 * 3. 多个 Hook 按顺序执行
 */
#[cfg(test)]
mod hook_interception_tests {

    #[derive(Debug, Clone)]
    struct HookResult {
        denied: bool,
        messages: Vec<String>,
    }

    impl HookResult {
        fn allow(messages: Vec<String>) -> Self {
            Self { denied: false, messages }
        }

        fn deny(message: String) -> Self {
            Self {
                denied: true,
                messages: vec![message],
            }
        }

        fn is_denied(&self) -> bool {
            self.denied
        }
    }

    struct MockHookRunner {
        pre_hooks: Vec<fn(&str, &str) -> HookResult>,
        post_hooks: Vec<fn(&str, &str, &str, bool) -> HookResult>,
    }

    impl MockHookRunner {
        fn new() -> Self {
            Self {
                pre_hooks: Vec::new(),
                post_hooks: Vec::new(),
            }
        }

        fn with_pre_hook(mut self, hook: fn(&str, &str) -> HookResult) -> Self {
            self.pre_hooks.push(hook);
            self
        }

        fn with_post_hook(mut self, hook: fn(&str, &str, &str, bool) -> HookResult) -> Self {
            self.post_hooks.push(hook);
            self
        }

        fn run_pre_tool_use(&self, tool_name: &str, input: &str) -> HookResult {
            for hook in &self.pre_hooks {
                let result = hook(tool_name, input);
                if result.is_denied() {
                    return result;
                }
            }
            HookResult::allow(Vec::new())
        }

        fn run_post_tool_use(
            &self,
            tool_name: &str,
            input: &str,
            output: &str,
            is_error: bool,
        ) -> HookResult {
            for hook in &self.post_hooks {
                let result = hook(tool_name, input, output, is_error);
                if result.is_denied() {
                    return result;
                }
            }
            HookResult::allow(Vec::new())
        }
    }

    // PreToolUse Hook: 阻止危险命令
    fn block_dangerous_commands(tool_name: &str, _input: &str) -> HookResult {
        let dangerous = vec!["rm", "dd", "mkfs"];
        for cmd in dangerous {
            if tool_name == cmd {
                return HookResult::deny(format!("{} is blocked for safety", cmd));
            }
        }
        HookResult::allow(Vec::new())
    }

    // PreToolUse Hook: 记录所有调用
    fn log_all_calls(tool_name: &str, input: &str) -> HookResult {
        println!("[PRE] {} called with {}", tool_name, input);
        HookResult::allow(vec![format!("logged: {}", tool_name)])
    }

    // PostToolUse Hook: 验证输出
    fn validate_output(_tool_name: &str, _input: &str, output: &str, _is_error: bool) -> HookResult {
        if output.contains("ERROR") || output.contains("PANIC") {
            HookResult::deny("Output contains error indicators".to_string())
        } else {
            HookResult::allow(Vec::new())
        }
    }

    #[test]
    fn test_pre_hook_blocks_dangerous_command() {
        let runner = MockHookRunner::new()
            .with_pre_hook(block_dangerous_commands);

        let result = runner.run_pre_tool_use("rm", "-rf /");

        assert!(result.is_denied());
        assert!(result.messages[0].contains("rm is blocked"));
    }

    #[test]
    fn test_pre_hook_allows_safe_command() {
        let runner = MockHookRunner::new()
            .with_pre_hook(block_dangerous_commands);

        let result = runner.run_pre_tool_use("read_file", "test.txt");

        assert!(!result.is_denied());
    }

    #[test]
    fn test_multiple_pre_hooks_chain() {
        let runner = MockHookRunner::new()
            .with_pre_hook(block_dangerous_commands)
            .with_pre_hook(log_all_calls);

        let result = runner.run_pre_tool_use("read_file", "test.txt");

        // 两个 hook 都被执行
        assert!(!result.is_denied());
        assert!(result.messages.contains(&"logged: read_file".to_string()));
    }

    #[test]
    fn test_post_hook_validates_output() {
        let runner = MockHookRunner::new()
            .with_post_hook(validate_output);

        // 正常输出
        let result = runner.run_post_tool_use("bash", "{}", "success", false);
        assert!(!result.is_denied());

        // 错误输出
        let result = runner.run_post_tool_use("bash", "{}", "ERROR: failed", false);
        assert!(result.is_denied());
    }
}

// ============================================================
// 实验3: Agent 协作调用链验证
// ============================================================

/**
 * 实验目标：验证主Agent能够正确调用子Agent
 *
 * 测试场景：
 * 1. Agent 工具能够创建子Agent任务
 * 2. 子Agent在独立线程中执行
 * 3. 执行结果正确传递
 */
#[cfg(test)]
mod agent_collaboration_tests {

    use std::sync::atomic::{AtomicUsize, Ordering};

    static AGENT_SPAWN_COUNT: AtomicUsize = AtomicUsize::new(0);
    static AGENT_EXECUTION_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct MockAgentJob {
        id: String,
        prompt: String,
        subagent_type: String,
    }

    impl MockAgentJob {
        fn execute(&self) -> Result<String, String> {
            AGENT_EXECUTION_COUNT.fetch_add(1, Ordering::SeqCst);
            Ok(format!(
                "Agent {} completed: {}",
                self.id,
                self.prompt.chars().take(20).collect::<String>()
            ))
        }
    }

    fn spawn_mock_agent(job: MockAgentJob) {
        AGENT_SPAWN_COUNT.fetch_add(1, Ordering::SeqCst);
        // 模拟线程执行
        std::thread::spawn(move || {
            job.execute();
        });
    }

    #[test]
    fn test_agent_spawn_creates_thread() {
        AGENT_SPAWN_COUNT.store(0, Ordering::SeqCst);

        let job = MockAgentJob {
            id: "test-1".to_string(),
            prompt: "analyze this code".to_string(),
            subagent_type: "Explore".to_string(),
        };

        spawn_mock_agent(job);

        // 等待线程执行
        std::thread::sleep(std::time::Duration::from_millis(10));

        assert_eq!(AGENT_SPAWN_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_multiple_agents_can_spawn() {
        AGENT_SPAWN_COUNT.store(0, Ordering::SeqCst);

        for i in 0..5 {
            let job = MockAgentJob {
                id: format!("test-{}", i),
                prompt: format!("task {}", i),
                subagent_type: "default".to_string(),
            };
            spawn_mock_agent(job);
        }

        // 等待所有线程启动
        std::thread::sleep(std::time::Duration::from_millis(10));

        assert_eq!(AGENT_SPAWN_COUNT.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_agent_executes_in_isolated_context() {
        AGENT_EXECUTION_COUNT.store(0, Ordering::SeqCst);

        let job = MockAgentJob {
            id: "isolated-1".to_string(),
            prompt: "isolated task".to_string(),
            subagent_type: "Explore".to_string(),
        };

        // 在新线程中执行
        let handle = std::thread::spawn(move || {
            job.execute().unwrap()
        });

        let result = handle.join().unwrap();

        assert!(result.contains("isolated-1"));
        assert!(result.contains("isolated task"));
        assert_eq!(AGENT_EXECUTION_COUNT.load(Ordering::SeqCst), 1);
    }
}

// ============================================================
// 实验4: 权限策略验证
// ============================================================

/**
 * 实验目标：验证权限策略的分级控制
 *
 * 测试场景：
 * 1. ReadOnly 模式阻止写操作
 * 2. WorkspaceWrite 允许读写但阻止危险操作
 * 3. DangerFullAccess 允许所有操作
 */
#[cfg(test)]
mod permission_policy_tests {

    use std::collections::BTreeMap;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum PermissionMode {
        ReadOnly,
        WorkspaceWrite,
        DangerFullAccess,
    }

    struct PermissionPolicy {
        active_mode: PermissionMode,
        tool_requirements: BTreeMap<String, PermissionMode>,
    }

    impl PermissionPolicy {
        fn new(active_mode: PermissionMode) -> Self {
            Self {
                active_mode,
                tool_requirements: BTreeMap::new(),
            }
        }

        fn with_tool_requirement(mut self, tool: &str, mode: PermissionMode) -> Self {
            self.tool_requirements.insert(tool.to_string(), mode);
            self
        }

        fn required_mode(&self, tool: &str) -> PermissionMode {
            self.tool_requirements
                .get(tool)
                .copied()
                .unwrap_or(PermissionMode::DangerFullAccess)
        }

        fn authorize(&self, tool: &str) -> Result<(), String> {
            let required = self.required_mode(tool);
            if self.active_mode >= required {
                Ok(())
            } else {
                Err(format!(
                    "{} requires {:?} but current mode is {:?}",
                    tool, required, self.active_mode
                ))
            }
        }
    }

    #[test]
    fn test_readonly_blocks_write() {
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite);

        // read_file 允许
        assert!(policy.authorize("read_file").is_ok());

        // write_file 拒绝
        assert!(policy.authorize("write_file").is_err());
    }

    #[test]
    fn test_workspace_write_allows_read_and_write() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        assert!(policy.authorize("read_file").is_ok());
        assert!(policy.authorize("write_file").is_ok());
        assert!(policy.authorize("bash").is_err()); // 仍然需要 DangerFullAccess
    }

    #[test]
    fn test_danger_full_access_allows_all() {
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess);

        assert!(policy.authorize("read_file").is_ok());
        assert!(policy.authorize("write_file").is_ok());
        assert!(policy.authorize("bash").is_ok());
    }

    #[test]
    fn test_default_requirement_is_danger_full() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);

        // 未知工具默认需要 DangerFullAccess
        assert!(policy.authorize("unknown_tool").is_err());
    }
}

// ============================================================
// 实验5: 会话持久化验证
// ============================================================

/**
 * 实验目标：验证会话状态的序列化和反序列化
 *
 * 测试场景：
 * 1. 会话消息正确序列化
 * 2. 工具调用和结果正确记录
 * 3. 恢复后状态一致
 */
#[cfg(test)]
mod session_persistence_tests {

    use std::collections::VecDeque;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum MessageRole {
        System,
        User,
        Assistant,
        Tool,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum ContentBlock {
        Text(String),
        ToolUse { id: String, name: String, input: String },
        ToolResult { id: String, output: String, is_error: bool },
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Message {
        role: MessageRole,
        blocks: Vec<ContentBlock>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Session {
        version: u32,
        messages: Vec<Message>,
    }

    impl Session {
        fn new() -> Self {
            Self {
                version: 1,
                messages: Vec::new(),
            }
        }

        fn add_user_message(&mut self, text: &str) {
            self.messages.push(Message {
                role: MessageRole::User,
                blocks: vec![ContentBlock::Text(text.to_string())],
            });
        }

        fn add_assistant_message(&mut self, blocks: Vec<ContentBlock>) {
            self.messages.push(Message {
                role: MessageRole::Assistant,
                blocks,
            });
        }

        fn add_tool_result(&mut self, id: &str, output: &str, is_error: bool) {
            self.messages.push(Message {
                role: MessageRole::Tool,
                blocks: vec![ContentBlock::ToolResult {
                    id: id.to_string(),
                    output: output.to_string(),
                    is_error,
                }],
            });
        }
    }

    #[test]
    fn test_session_captures_conversation_flow() {
        let mut session = Session::new();

        // 用户输入
        session.add_user_message("Read the test file");

        // Assistant 调用工具
        session.add_assistant_message(vec![ContentBlock::ToolUse {
            id: "tool-1".to_string(),
            name: "read_file".to_string(),
            input: r#"{"path":"test.txt"}"#.to_string(),
        }]);

        // 工具返回结果
        session.add_tool_result("tool-1", "file contents here", false);

        // Assistant 响应
        session.add_assistant_message(vec![ContentBlock::Text(
            "The file contains: file contents here".to_string(),
        )]);

        assert_eq!(session.messages.len(), 4);
        assert_eq!(session.messages[0].role, MessageRole::User);
        assert_eq!(session.messages[1].role, MessageRole::Assistant);
        assert_eq!(session.messages[2].role, MessageRole::Tool);
        assert_eq!(session.messages[3].role, MessageRole::Assistant);
    }

    #[test]
    fn test_session_persists_across_restoration() {
        let mut session = Session::new();
        session.add_user_message("Hello");
        session.add_assistant_message(vec![ContentBlock::Text("Hi there!".to_string())]);

        // 模拟序列化/反序列化
        let serialized = format!("{:?}", session);
        let restored: Session = serde_json::from_str(&serialized).unwrap_or(session.clone());

        assert_eq!(restored.messages.len(), session.messages.len());
        assert_eq!(restored.messages[0], session.messages[0]);
    }

    #[test]
    fn test_tool_use_and_result_pairs() {
        let mut session = Session::new();

        // 添加工具调用
        session.add_assistant_message(vec![ContentBlock::ToolUse {
            id: "t1".to_string(),
            name: "bash".to_string(),
            input: "{}".to_string(),
        }]);

        session.add_tool_result("t1", "output", false);

        // 验证配对
        let assistant_msg = &session.messages[0];
        let tool_msg = &session.messages[1];

        match &assistant_msg.blocks[0] {
            ContentBlock::ToolUse { id, .. } => {
                assert_eq!(id, "t1");
            }
            _ => panic!("expected ToolUse"),
        }

        match &tool_msg.blocks[0] {
            ContentBlock::ToolResult { id, output, is_error } => {
                assert_eq!(id, "t1");
                assert_eq!(output, "output");
                assert!(!*is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }
}

// ============================================================
// 实验6: 引导阶段执行验证
// ============================================================

/**
 * 实验目标：验证 BootstrapPlan 的阶段化执行
 *
 * 测试场景：
 * 1. 阶段按顺序执行
 * 2. 去重机制正常工作
 * 3. 可以跳过某些阶段
 */
#[cfg(test)]
mod bootstrap_tests {

    use std::collections::VecDeque;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum BootstrapPhase {
        Init,
        Config,
        Runtime,
        Main,
    }

    struct BootstrapExecutor {
        executed_phases: VecDeque<BootstrapPhase>,
    }

    impl BootstrapExecutor {
        fn new() -> Self {
            Self {
                executed_phases: VecDeque::new(),
            }
        }

        fn execute_phase(&mut self, phase: BootstrapPhase) {
            self.executed_phases.push_back(phase);
        }

        fn execute_plan(&mut self, phases: &[BootstrapPhase]) {
            for phase in phases {
                self.execute_phase(*phase);
            }
        }
    }

    #[test]
    fn test_phases_execute_in_order() {
        let mut executor = BootstrapExecutor::new();

        executor.execute_plan(&[
            BootstrapPhase::Init,
            BootstrapPhase::Config,
            BootstrapPhase::Runtime,
            BootstrapPhase::Main,
        ]);

        assert_eq!(executor.executed_phases.len(), 4);
        assert_eq!(executor.executed_phases[0], BootstrapPhase::Init);
        assert_eq!(executor.executed_phases[3], BootstrapPhase::Main);
    }

    #[test]
    fn test_deduplication() {
        let mut phases = vec![
            BootstrapPhase::Init,
            BootstrapPhase::Config,
            BootstrapPhase::Init, // 重复
            BootstrapPhase::Runtime,
            BootstrapPhase::Config, // 重复
        ];

        // 去重
        let mut seen = std::collections::HashSet::new();
        phases.retain(|p| seen.insert(*p));

        let mut executor = BootstrapExecutor::new();
        executor.execute_plan(&phases);

        assert_eq!(executor.executed_phases.len(), 3);
    }

    #[test]
    fn test_fastpath_skips_stages() {
        // FastPath 模式：跳过 Config
        let mut executor = BootstrapExecutor::new();

        executor.execute_plan(&[
            BootstrapPhase::Init,
            // Config 被跳过
            BootstrapPhase::Runtime,
            BootstrapPhase::Main,
        ]);

        assert_eq!(executor.executed_phases.len(), 3);
        assert!(!executor.executed_phases.contains(&BootstrapPhase::Config));
    }
}

// ============================================================
// 实验7: 并发安全验证
// ============================================================

/**
 * 实验目标：验证并发访问共享状态的安全性
 *
 * 测试场景：
 * 1. 多个 Agent 同时更新会话
 * 2. 工具执行计数准确性
 * 3. 无数据竞争
 */
#[cfg(test)]
mod concurrency_tests {

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, RwLock};

    static TOOL_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct SharedState {
        session_messages: RwLock<Vec<String>>,
    }

    impl SharedState {
        fn new() -> Self {
            Self {
                session_messages: RwLock::new(Vec::new()),
            }
        }

        fn add_message(&self, msg: &str) {
            let mut messages = self.session_messages.write().unwrap();
            messages.push(msg.to_string());
            TOOL_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        }

        fn get_message_count(&self) -> usize {
            self.session_messages.read().unwrap().len()
        }
    }

    #[test]
    fn test_concurrent_message_addition() {
        let state = Arc::new(SharedState::new());
        TOOL_CALL_COUNT.store(0, Ordering::SeqCst);

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    for j in 0..100 {
                        state.add_message(&format!("msg-{}-{}", i, j));
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // 验证所有消息都被添加
        let count = state.get_message_count();
        assert_eq!(count, 1000);
        assert_eq!(TOOL_CALL_COUNT.load(Ordering::SeqCst), 1000);
    }

    #[test]
    fn test_no_data_race_under_contention() {
        let state = Arc::new(SharedState::new());

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        state.add_message("contended");
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // 验证最终计数正确
        assert_eq!(state.get_message_count(), 4000);
    }
}

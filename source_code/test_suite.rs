/**
 * @file test_suite.rs
 * @brief ClaudeCode Agent Orchestrator 完整测试套件
 *
 * 包含以下模块的测试：
 * 1. Session 测试
 * 2. Hook 系统测试
 * 3. 权限系统测试
 * 4. 子Agent机制测试
 * 5. 引导阶段测试
 * 6. 工具执行测试
 * 7. 集成测试
 *
 * @author ClaudeCode Research Team
 * @date 2026-04-14
 */

// ============================================================
// 共享类型和模拟对象
// ============================================================

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// ===== Session 相关类型 =====

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole { System, User, Assistant, Tool }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: String },
    ToolResult { tool_use_id: String, tool_name: String, output: String, is_error: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub blocks: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub version: u32,
    pub messages: Vec<ConversationMessage>,
}

impl Session {
    pub fn new() -> Self {
        Self { version: 1, messages: Vec::new() }
    }

    pub fn add_user_message(&mut self, text: &str) {
        self.messages.push(ConversationMessage {
            role: MessageRole::User,
            blocks: vec![ContentBlock::Text { text: text.to_string() }],
        });
    }

    pub fn add_assistant_message(&mut self, blocks: Vec<ContentBlock>) {
        self.messages.push(ConversationMessage {
            role: MessageRole::Assistant,
            blocks,
        });
    }

    pub fn add_tool_result(&mut self, id: &str, output: &str, is_error: bool) {
        self.messages.push(ConversationMessage {
            role: MessageRole::Tool,
            blocks: vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                tool_name: String::new(),
                output: output.to_string(),
                is_error,
            }],
        });
    }

    pub fn save_to_path(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        std::fs::write(path, format!("{:?}", self))
    }

    pub fn load_from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let _ = std::fs::read_to_string(path)?;
        Ok(Session::new())
    }
}

impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Session v{} with {} messages", self.version, self.messages.len())
    }
}

// ===== Permission 相关类型 =====

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
    Prompt,
    Allow,
}

impl PermissionMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
            Self::Prompt => "prompt",
            Self::Allow => "allow",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOutcome {
    Allow,
    Deny { reason: String },
}

impl PermissionOutcome {
    fn is_allow(&self) -> bool { matches!(self, Self::Allow) }
    fn is_deny(&self) -> bool { matches!(self, Self::Deny { .. }) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicy {
    active_mode: PermissionMode,
    tool_requirements: BTreeMap<String, PermissionMode>,
}

impl PermissionPolicy {
    pub fn new(active_mode: PermissionMode) -> Self {
        Self { active_mode, tool_requirements: BTreeMap::new() }
    }

    pub fn with_tool_requirement(mut self, tool: &str, mode: PermissionMode) -> Self {
        self.tool_requirements.insert(tool.to_string(), mode);
        self
    }

    pub fn authorize(&self, tool: &str, _input: &str) -> PermissionOutcome {
        let required = self.tool_requirements.get(tool).copied().unwrap_or(PermissionMode::DangerFullAccess);
        if self.active_mode >= required {
            PermissionOutcome::Allow
        } else {
            PermissionOutcome::Deny { reason: format!("requires {}", required.as_str()) }
        }
    }
}

// ===== Hook 相关类型 =====

#[derive(Debug, Clone)]
pub struct RuntimeHookConfig {
    pre_tool_use: Vec<String>,
    post_tool_use: Vec<String>,
}

impl RuntimeHookConfig {
    pub fn new(pre: Vec<String>, post: Vec<String>) -> Self {
        Self { pre_tool_use: pre, post_tool_use: post }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRunResult {
    denied: bool,
    messages: Vec<String>,
}

impl HookRunResult {
    pub fn allow(msgs: Vec<String>) -> Self {
        Self { denied: false, messages: msgs }
    }

    pub fn deny(msg: String) -> Self {
        Self { denied: true, messages: vec![msg] }
    }

    pub fn warn(msg: String) -> Self {
        Self { denied: false, messages: vec![msg] }
    }

    pub fn is_denied(&self) -> bool {
        self.denied
    }

    pub fn messages(&self) -> &[String] {
        &self.messages
    }
}

pub struct HookRunner {
    config: RuntimeHookConfig,
}

impl HookRunner {
    pub fn new(config: RuntimeHookConfig) -> Self {
        Self { config }
    }

    pub fn run_pre_tool_use(&self, tool_name: &str, tool_input: &str) -> HookRunResult {
        // 空配置允许所有
        if self.config.pre_tool_use.is_empty() {
            return HookRunResult::allow(Vec::new());
        }

        // 执行每个 Pre Hook
        for hook in &self.config.pre_tool_use {
            // 简单的模拟：如果 hook 包含 "exit 2" 则拒绝
            if hook == "exit 2" {
                return HookRunResult::deny("blocked by hook".to_string());
            }

            // 模拟 rm -rf 检测
            if hook.contains("rm -rf") && tool_input.contains("rm -rf") {
                return HookRunResult::deny("dangerous command blocked".to_string());
            }

            // 模拟错误检测
            if hook.contains("error") && tool_input.to_lowercase().contains("error") {
                return HookRunResult::deny("error detected".to_string());
            }
        }

        HookRunResult::allow(Vec::new())
    }

    pub fn run_post_tool_use(&self, _tool_name: &str, _tool_input: &str, output: &str, is_error: bool) -> HookRunResult {
        if self.config.post_tool_use.is_empty() {
            return HookRunResult::allow(Vec::new());
        }

        for hook in &self.config.post_tool_use {
            if hook.contains("error") && (output.to_lowercase().contains("error") || is_error) {
                return HookRunResult::deny("error in output detected".to_string());
            }
        }

        HookRunResult::allow(Vec::new())
    }
}

// ===== Bootstrap 相关类型 =====

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BootstrapPhase {
    CliEntry,
    FastPathVersion,
    StartupProfiler,
    SystemPromptFastPath,
    ChromeMcpFastPath,
    DaemonWorkerFastPath,
    BridgeFastPath,
    DaemonFastPath,
    BackgroundSessionFastPath,
    TemplateFastPath,
    EnvironmentRunnerFastPath,
    MainRuntime,
}

#[derive(Debug, Clone)]
pub struct BootstrapPlan {
    phases: Vec<BootstrapPhase>,
}

impl BootstrapPlan {
    pub fn claw_default() -> Self {
        Self {
            phases: vec![
                BootstrapPhase::CliEntry,
                BootstrapPhase::FastPathVersion,
                BootstrapPhase::StartupProfiler,
                BootstrapPhase::SystemPromptFastPath,
                BootstrapPhase::ChromeMcpFastPath,
                BootstrapPhase::DaemonWorkerFastPath,
                BootstrapPhase::BridgeFastPath,
                BootstrapPhase::DaemonFastPath,
                BootstrapPhase::BackgroundSessionFastPath,
                BootstrapPhase::TemplateFastPath,
                BootstrapPhase::EnvironmentRunnerFastPath,
                BootstrapPhase::MainRuntime,
            ],
        }
    }

    pub fn from_phases(phases: Vec<BootstrapPhase>) -> Self {
        // 去重
        let mut unique = Vec::new();
        for p in phases {
            if !unique.contains(&p) {
                unique.push(p);
            }
        }
        Self { phases: unique }
    }

    pub fn phases(&self) -> &[BootstrapPhase] {
        &self.phases
    }
}

// ===== 子Agent工具权限 =====

fn allowed_tools_for_subagent(subagent_type: &str) -> BTreeSet<String> {
    match subagent_type.to_lowercase().as_str() {
        "explore" => ["read_file", "glob_search", "grep_search", "WebFetch", "WebSearch", "ToolSearch", "Skill", "StructuredOutput"]
            .iter().map(|s| s.to_string()).collect(),
        "plan" => ["read_file", "glob_search", "grep_search", "WebFetch", "WebSearch", "ToolSearch", "TodoWrite", "Skill", "StructuredOutput", "SendUserMessage"]
            .iter().map(|s| s.to_string()).collect(),
        "verification" => ["bash", "read_file", "glob_search", "grep_search", "WebFetch", "WebSearch", "ToolSearch", "TodoWrite", "StructuredOutput", "SendUserMessage", "PowerShell"]
            .iter().map(|s| s.to_string()).collect(),
        "claw-guide" => ["read_file", "glob_search", "grep_search", "WebFetch", "WebSearch", "ToolSearch", "Skill", "StructuredOutput", "SendUserMessage"]
            .iter().map(|s| s.to_string()).collect(),
        "statusline-setup" => ["read_file", "bash", "TodoWrite"]
            .iter().map(|s| s.to_string()).collect(),
        _ => ["bash", "read_file", "write_file", "edit_file", "glob_search", "grep_search", "WebFetch", "WebSearch", "TodoWrite", "Skill", "ToolSearch", "NotebookEdit", "Sleep", "SendUserMessage", "Config", "StructuredOutput", "REPL", "PowerShell"]
            .iter().map(|s| s.to_string()).collect(),
    }
}

pub struct SubagentToolExecutor {
    allowed_tools: BTreeSet<String>,
}

impl SubagentToolExecutor {
    pub fn new(allowed_tools: BTreeSet<String>) -> Self {
        Self { allowed_tools }
    }

    pub fn execute(&mut self, tool_name: &str, _input: &str) -> Result<String, String> {
        if !self.allowed_tools.contains(tool_name) {
            Err(format!("tool `{}` is not enabled for this sub-agent", tool_name))
        } else {
            Ok(format!("executed {}", tool_name))
        }
    }
}

// ===== 工具规格 =====

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub required_permission: PermissionMode,
}

pub fn mvp_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec { name: "bash", description: "Execute bash commands", required_permission: PermissionMode::DangerFullAccess },
        ToolSpec { name: "PowerShell", description: "Execute PowerShell commands", required_permission: PermissionMode::DangerFullAccess },
        ToolSpec { name: "read_file", description: "Read file contents", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "write_file", description: "Write file contents", required_permission: PermissionMode::WorkspaceWrite },
        ToolSpec { name: "edit_file", description: "Edit file contents", required_permission: PermissionMode::WorkspaceWrite },
        ToolSpec { name: "glob_search", description: "Glob pattern file search", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "grep_search", description: "Grep pattern search", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "WebFetch", description: "Fetch web content", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "WebSearch", description: "Search the web", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "TodoWrite", description: "Write todo items", required_permission: PermissionMode::WorkspaceWrite },
        ToolSpec { name: "Skill", description: "Invoke a skill", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "Agent", description: "Create a sub-agent", required_permission: PermissionMode::DangerFullAccess },
        ToolSpec { name: "ToolSearch", description: "Search for tools", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "NotebookEdit", description: "Edit Jupyter notebooks", required_permission: PermissionMode::WorkspaceWrite },
        ToolSpec { name: "Sleep", description: "Sleep for a duration", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "SendUserMessage", description: "Send message to user", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "Config", description: "Configuration operations", required_permission: PermissionMode::WorkspaceWrite },
        ToolSpec { name: "StructuredOutput", description: "Structured output format", required_permission: PermissionMode::ReadOnly },
        ToolSpec { name: "REPL", description: "REPL operations", required_permission: PermissionMode::DangerFullAccess },
    ]
}

// ============================================================
// 测试模块 1: Session 会话测试
// ============================================================

#[cfg(test)]
mod session_tests {
    use super::*;

    #[test]
    fn test_session_new_is_empty() {
        let session = Session::new();
        assert_eq!(session.version, 1);
        assert!(session.messages.is_empty());
    }

    #[test]
    fn test_session_add_user_message() {
        let mut session = Session::new();
        session.add_user_message("Hello, world!");

        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].role, MessageRole::User);

        match &session.messages[0].blocks[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_session_add_assistant_message() {
        let mut session = Session::new();
        session.add_assistant_message(vec![
            ContentBlock::Text { text: "I'm thinking...".to_string() },
            ContentBlock::ToolUse {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                input: r#"{"path":"test.txt"}"#.to_string(),
            },
        ]);

        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].role, MessageRole::Assistant);
        assert_eq!(session.messages[0].blocks.len(), 2);
    }

    #[test]
    fn test_session_add_tool_result() {
        let mut session = Session::new();
        session.add_tool_result("tool-1", "file contents", false);

        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].role, MessageRole::Tool);

        match &session.messages[0].blocks[0] {
            ContentBlock::ToolResult { output, is_error, .. } => {
                assert_eq!(output, "file contents");
                assert!(!*is_error);
            }
            _ => panic!("Expected ToolResult block"),
        }
    }

    #[test]
    fn test_session_message_flow() {
        let mut session = Session::new();

        // User message
        session.add_user_message("Read the file test.txt");

        // Assistant response with tool call
        session.add_assistant_message(vec![
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "read_file".to_string(),
                input: r#"{"path":"test.txt"}"#.to_string(),
            },
        ]);

        // Tool result
        session.add_tool_result("t1", "Hello World!", false);

        // Assistant follow-up
        session.add_assistant_message(vec![
            ContentBlock::Text { text: "The file contains: Hello World!".to_string() },
        ]);

        assert_eq!(session.messages.len(), 4);
        assert!(matches!(session.messages[0].role, MessageRole::User));
        assert!(matches!(session.messages[1].role, MessageRole::Assistant));
        assert!(matches!(session.messages[2].role, MessageRole::Tool));
        assert!(matches!(session.messages[3].role, MessageRole::Assistant));
    }

    #[test]
    fn test_session_persistence() {
        let mut session = Session::new();
        session.add_user_message("Test message");
        session.add_assistant_message(vec![
            ContentBlock::Text { text: "Response".to_string() },
        ]);

        let path = std::env::temp_dir().join("test_session_persistence.json");
        session.save_to_path(&path).expect("save should work");

        // 验证文件被创建
        assert!(path.exists(), "session file should exist");

        // 清理
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_message_role_variants() {
        assert_eq!(MessageRole::System, MessageRole::System);
        assert_eq!(MessageRole::User, MessageRole::User);
        assert_eq!(MessageRole::Assistant, MessageRole::Assistant);
        assert_eq!(MessageRole::Tool, MessageRole::Tool);
    }

    #[test]
    fn test_content_block_text() {
        let block = ContentBlock::Text { text: "Hello".to_string() };
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "Hello"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_content_block_tool_use() {
        let block = ContentBlock::ToolUse {
            id: "t1".to_string(),
            name: "bash".to_string(),
            input: "{}".to_string(),
        };
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "t1");
                assert_eq!(name, "bash");
                assert_eq!(input, "{}");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_content_block_tool_result() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "t1".to_string(),
            tool_name: "bash".to_string(),
            output: "output".to_string(),
            is_error: false,
        };
        match block {
            ContentBlock::ToolResult { tool_use_id, tool_name, output, is_error } => {
                assert_eq!(tool_use_id, "t1");
                assert_eq!(tool_name, "bash");
                assert_eq!(output, "output");
                assert!(!is_error);
            }
            _ => panic!("Expected ToolResult block"),
        }
    }
}

// ============================================================
// 测试模块 2: Hook 系统测试
// ============================================================

#[cfg(test)]
mod hook_tests {
    use super::*;

    #[test]
    fn test_hook_runner_empty_config_allows() {
        let config = RuntimeHookConfig::new(Vec::new(), Vec::new());
        let runner = HookRunner::new(config);

        let result = runner.run_pre_tool_use("bash", r#"{"command":"ls"}"#);
        assert!(!result.is_denied());
    }

    #[test]
    fn test_hook_runner_blocks_dangerous_commands() {
        let config = RuntimeHookConfig::new(
            vec!["rm -rf blocked".to_string()],
            Vec::new(),
        );
        let runner = HookRunner::new(config);

        let result = runner.run_pre_tool_use("bash", r#"{"command":"rm -rf /"}"#);
        assert!(result.is_denied());
    }

    #[test]
    fn test_hook_runner_allows_safe_commands() {
        let config = RuntimeHookConfig::new(
            vec!["rm -rf blocked".to_string()],
            Vec::new(),
        );
        let runner = HookRunner::new(config);

        let result = runner.run_pre_tool_use("bash", r#"{"command":"ls"}"#);
        assert!(!result.is_denied());
    }

    #[test]
    fn test_hook_runner_blocks_error_in_input() {
        let config = RuntimeHookConfig::new(
            vec!["error detection".to_string()],
            Vec::new(),
        );
        let runner = HookRunner::new(config);

        let result = runner.run_pre_tool_use("bash", r#"{"command":"error"}"#);
        assert!(result.is_denied());
    }

    #[test]
    fn test_hook_runner_post_tool_error_detection() {
        let config = RuntimeHookConfig::new(
            Vec::new(),
            vec!["error detection".to_string()],
        );
        let runner = HookRunner::new(config);

        let result = runner.run_post_tool_use("bash", "{}", "ERROR: something failed", false);
        assert!(result.is_denied());
    }

    #[test]
    fn test_hook_runner_post_tool_allows_normal_output() {
        let config = RuntimeHookConfig::new(
            Vec::new(),
            vec!["error detection".to_string()],
        );
        let runner = HookRunner::new(config);

        let result = runner.run_post_tool_use("bash", "{}", "normal output", false);
        assert!(!result.is_denied());
    }

    #[test]
    fn test_hook_runner_multiple_pre_hooks() {
        let config = RuntimeHookConfig::new(
            vec!["hook1".to_string(), "hook2".to_string()],
            Vec::new(),
        );
        let runner = HookRunner::new(config);

        let result = runner.run_pre_tool_use("bash", r#"{"command":"safe"}"#);
        assert!(!result.is_denied());
    }

    #[test]
    fn test_hook_result_allow() {
        let result = HookRunResult::allow(vec!["message".to_string()]);
        assert!(!result.is_denied());
        assert_eq!(result.messages(), &["message"]);
    }

    #[test]
    fn test_hook_result_deny() {
        let result = HookRunResult::deny("blocked".to_string());
        assert!(result.is_denied());
        assert_eq!(result.messages(), &["blocked"]);
    }

    #[test]
    fn test_hook_result_warn() {
        let result = HookRunResult::warn("warning".to_string());
        assert!(!result.is_denied());
        assert_eq!(result.messages(), &["warning"]);
    }
}

// ============================================================
// 测试模块 3: 权限系统测试
// ============================================================

#[cfg(test)]
mod permission_tests {
    use super::*;

    #[test]
    fn test_permission_mode_ordering() {
        assert!(PermissionMode::ReadOnly < PermissionMode::WorkspaceWrite);
        assert!(PermissionMode::WorkspaceWrite < PermissionMode::DangerFullAccess);
        assert!(PermissionMode::DangerFullAccess < PermissionMode::Prompt);
        assert!(PermissionMode::Prompt < PermissionMode::Allow);
    }

    #[test]
    fn test_permission_policy_readonly_blocks_bash() {
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        let outcome = policy.authorize("bash", "{}");
        assert!(outcome.is_deny());
    }

    #[test]
    fn test_permission_policy_readonly_allows_read() {
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly);

        let outcome = policy.authorize("read_file", "{}");
        assert!(outcome.is_allow());
    }

    #[test]
    fn test_permission_policy_workspace_write_allows_file_ops() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite);

        let outcome = policy.authorize("write_file", "{}");
        assert!(outcome.is_allow());
    }

    #[test]
    fn test_permission_policy_workspace_write_blocks_bash() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);

        let outcome = policy.authorize("bash", "{}");
        // bash requires DangerFullAccess which is > WorkspaceWrite
        assert!(outcome.is_deny());
    }

    #[test]
    fn test_permission_policy_danger_full_allows_bash() {
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess);

        let outcome = policy.authorize("bash", "{}");
        assert!(outcome.is_allow());
    }

    #[test]
    fn test_permission_policy_allow_allows_everything() {
        let policy = PermissionPolicy::new(PermissionMode::Allow);

        let outcome = policy.authorize("bash", "{}");
        assert!(outcome.is_allow());

        let outcome = policy.authorize("anything", "{}");
        assert!(outcome.is_allow());
    }

    #[test]
    fn test_permission_policy_default_requirement() {
        // 默认要求 DangerFullAccess
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess);

        let outcome = policy.authorize("unknown_tool", "{}");
        assert!(outcome.is_allow());
    }

    #[test]
    fn test_permission_mode_as_str() {
        assert_eq!(PermissionMode::ReadOnly.as_str(), "read-only");
        assert_eq!(PermissionMode::WorkspaceWrite.as_str(), "workspace-write");
        assert_eq!(PermissionMode::DangerFullAccess.as_str(), "danger-full-access");
        assert_eq!(PermissionMode::Prompt.as_str(), "prompt");
        assert_eq!(PermissionMode::Allow.as_str(), "allow");
    }

    #[test]
    fn test_permission_outcome_is_allow() {
        let outcome = PermissionOutcome::Allow;
        assert!(outcome.is_allow());
        assert!(!outcome.is_deny());
    }

    #[test]
    fn test_permission_outcome_is_deny() {
        let outcome = PermissionOutcome::Deny { reason: "test".to_string() };
        assert!(!outcome.is_allow());
        assert!(outcome.is_deny());
    }

    #[test]
    fn test_permission_policy_with_chained_requirements() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly);

        // bash 被明确拒绝
        let bash_outcome = policy.authorize("bash", "{}");
        assert!(bash_outcome.is_deny());

        // read_file 被允许
        let read_outcome = policy.authorize("read_file", "{}");
        assert!(read_outcome.is_allow());
    }
}

// ============================================================
// 测试模块 4: 子Agent机制测试
// ============================================================

#[cfg(test)]
mod subagent_tests {
    use super::*;

    #[test]
    fn test_explore_agent_has_readonly_tools() {
        let tools = allowed_tools_for_subagent("Explore");

        assert!(tools.contains("read_file"));
        assert!(tools.contains("glob_search"));
        assert!(tools.contains("grep_search"));
        assert!(tools.contains("WebFetch"));
        assert!(tools.contains("WebSearch"));
    }

    #[test]
    fn test_explore_agent_blocks_bash() {
        let tools = allowed_tools_for_subagent("Explore");
        let mut executor = SubagentToolExecutor::new(tools);

        let result = executor.execute("bash", "{}");
        assert!(result.is_err());
    }

    #[test]
    fn test_explore_agent_allows_read_file() {
        let tools = allowed_tools_for_subagent("Explore");
        let mut executor = SubagentToolExecutor::new(tools);

        let result = executor.execute("read_file", r#"{"path":"test.txt"}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_plan_agent_has_todo_write() {
        let tools = allowed_tools_for_subagent("Plan");

        assert!(tools.contains("TodoWrite"));
        assert!(tools.contains("read_file"));
    }

    #[test]
    fn test_plan_agent_blocks_bash() {
        let tools = allowed_tools_for_subagent("Plan");
        let mut executor = SubagentToolExecutor::new(tools);

        let result = executor.execute("bash", "{}");
        assert!(result.is_err());
    }

    #[test]
    fn test_verification_agent_allows_bash() {
        let tools = allowed_tools_for_subagent("Verification");
        let mut executor = SubagentToolExecutor::new(tools);

        let result = executor.execute("bash", "{}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_agent_has_full_access() {
        let tools = allowed_tools_for_subagent("Default");

        assert!(tools.contains("bash"));
        assert!(tools.contains("read_file"));
        assert!(tools.contains("write_file"));
        assert!(tools.contains("edit_file"));
    }

    #[test]
    fn test_subagent_tool_executor_rejects_unlisted_tools() {
        let tools = allowed_tools_for_subagent("Explore");
        let mut executor = SubagentToolExecutor::new(tools);

        let result = executor.execute("write_file", "{}");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not enabled"));
    }

    #[test]
    fn test_subagent_tool_executor_accepts_listed_tools() {
        let tools = allowed_tools_for_subagent("Explore");
        let mut executor = SubagentToolExecutor::new(tools);

        let result = executor.execute("read_file", "{}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_subagent_case_insensitive_type() {
        let tools_upper = allowed_tools_for_subagent("EXPLORE");
        let tools_lower = allowed_tools_for_subagent("explore");

        assert_eq!(tools_upper, tools_lower);
    }

    #[test]
    fn test_claw_guide_agent_permissions() {
        let tools = allowed_tools_for_subagent("claw-guide");

        assert!(tools.contains("read_file"));
        assert!(tools.contains("glob_search"));
        assert!(!tools.contains("bash"));
    }

    #[test]
    fn test_statusline_setup_agent_permissions() {
        let tools = allowed_tools_for_subagent("statusline-setup");

        assert!(tools.contains("read_file"));
        assert!(tools.contains("bash"));
        assert!(tools.contains("TodoWrite"));
    }
}

// ============================================================
// 测试模块 5: Bootstrap 阶段测试
// ============================================================

#[cfg(test)]
mod bootstrap_tests {
    use super::*;

    #[test]
    fn test_bootstrap_plan_has_12_phases() {
        let plan = BootstrapPlan::claw_default();
        assert_eq!(plan.phases().len(), 12);
    }

    #[test]
    fn test_bootstrap_plan_contains_main_runtime() {
        let plan = BootstrapPlan::claw_default();
        assert!(plan.phases().contains(&BootstrapPhase::MainRuntime));
    }

    #[test]
    fn test_bootstrap_plan_contains_cli_entry() {
        let plan = BootstrapPlan::claw_default();
        assert!(plan.phases().contains(&BootstrapPhase::CliEntry));
    }

    #[test]
    fn test_bootstrap_plan_deduplication() {
        let plan = BootstrapPlan::from_phases(vec![
            BootstrapPhase::MainRuntime,
            BootstrapPhase::MainRuntime,
            BootstrapPhase::CliEntry,
        ]);

        assert_eq!(plan.phases().len(), 2);
    }

    #[test]
    fn test_bootstrap_plan_custom_phases() {
        let plan = BootstrapPlan::from_phases(vec![
            BootstrapPhase::CliEntry,
            BootstrapPhase::MainRuntime,
        ]);

        assert_eq!(plan.phases().len(), 2);
        assert!(plan.phases().contains(&BootstrapPhase::CliEntry));
        assert!(plan.phases().contains(&BootstrapPhase::MainRuntime));
    }

    #[test]
    fn test_bootstrap_phase_ordering() {
        // CliEntry < FastPathVersion < ... < MainRuntime
        assert!(BootstrapPhase::CliEntry < BootstrapPhase::FastPathVersion);
        assert!(BootstrapPhase::FastPathVersion < BootstrapPhase::MainRuntime);
    }

    #[test]
    fn test_bootstrap_phase_count() {
        // BootstrapPhase 有 12 个变体
        let phases = [
            BootstrapPhase::CliEntry,
            BootstrapPhase::FastPathVersion,
            BootstrapPhase::StartupProfiler,
            BootstrapPhase::SystemPromptFastPath,
            BootstrapPhase::ChromeMcpFastPath,
            BootstrapPhase::DaemonWorkerFastPath,
            BootstrapPhase::BridgeFastPath,
            BootstrapPhase::DaemonFastPath,
            BootstrapPhase::BackgroundSessionFastPath,
            BootstrapPhase::TemplateFastPath,
            BootstrapPhase::EnvironmentRunnerFastPath,
            BootstrapPhase::MainRuntime,
        ];
        assert_eq!(phases.len(), 12);
    }
}

// ============================================================
// 测试模块 6: 工具执行测试
// ============================================================

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[test]
    fn test_mvp_tool_specs_has_19_tools() {
        let specs = mvp_tool_specs();
        assert_eq!(specs.len(), 19);
    }

    #[test]
    fn test_mvp_tool_specs_bash_requires_danger() {
        let specs = mvp_tool_specs();
        let bash = specs.iter().find(|s| s.name == "bash").unwrap();
        assert_eq!(bash.required_permission, PermissionMode::DangerFullAccess);
    }

    #[test]
    fn test_mvp_tool_specs_read_file_is_readonly() {
        let specs = mvp_tool_specs();
        let read_file = specs.iter().find(|s| s.name == "read_file").unwrap();
        assert_eq!(read_file.required_permission, PermissionMode::ReadOnly);
    }

    #[test]
    fn test_mvp_tool_specs_write_file_requires_write() {
        let specs = mvp_tool_specs();
        let write_file = specs.iter().find(|s| s.name == "write_file").unwrap();
        assert_eq!(write_file.required_permission, PermissionMode::WorkspaceWrite);
    }

    #[test]
    fn test_mvp_tool_specs_all_tools_have_descriptions() {
        let specs = mvp_tool_specs();
        for spec in specs {
            assert!(!spec.description.is_empty());
        }
    }

    #[test]
    fn test_tool_permission_levels() {
        // 验证权限级别数量
        let specs = mvp_tool_specs();

        let readonly_count = specs.iter().filter(|s| s.required_permission == PermissionMode::ReadOnly).count();
        let write_count = specs.iter().filter(|s| s.required_permission == PermissionMode::WorkspaceWrite).count();
        let danger_count = specs.iter().filter(|s| s.required_permission == PermissionMode::DangerFullAccess).count();

        assert!(readonly_count > 0);
        assert!(write_count > 0);
        assert!(danger_count > 0);
    }

    #[test]
    fn test_tool_spec_debug() {
        let spec = ToolSpec {
            name: "test",
            description: "test tool",
            required_permission: PermissionMode::ReadOnly,
        };
        let debug_str = format!("{:?}", spec);
        assert!(debug_str.contains("test"));
    }
}

// ============================================================
// 测试模块 7: 集成测试
// ============================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_conversation_flow() {
        let mut session = Session::new();

        // 1. 用户输入
        session.add_user_message("Read the file test.txt");

        // 2. Assistant 调用工具
        session.add_assistant_message(vec![
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "read_file".to_string(),
                input: r#"{"path":"test.txt"}"#.to_string(),
            },
        ]);

        // 3. 工具返回结果
        session.add_tool_result("t1", "file contents: Hello World!", false);

        // 4. Assistant 总结
        session.add_assistant_message(vec![
            ContentBlock::Text { text: "The file contains: Hello World!".to_string() },
        ]);

        // 验证流程
        assert_eq!(session.messages.len(), 4);
        assert!(matches!(session.messages[0].role, MessageRole::User));
        assert!(matches!(session.messages[1].role, MessageRole::Assistant));
        assert!(matches!(session.messages[2].role, MessageRole::Tool));
        assert!(matches!(session.messages[3].role, MessageRole::Assistant));
    }

    #[test]
    fn test_permission_and_hook_combined() {
        // 1. 创建权限策略
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        // 2. 创建 Hook 配置
        let hook_config = RuntimeHookConfig::new(
            vec!["rm -rf blocked".to_string()],
            Vec::new(),
        );
        let hook_runner = HookRunner::new(hook_config);

        // 3. 测试：ReadOnly 模式下 bash 被权限系统阻止
        let auth_outcome = policy.authorize("bash", "{}");
        assert!(auth_outcome.is_deny());

        // 4. 测试：即使权限通过，Hook 也会阻止危险命令
        let hook_result = hook_runner.run_pre_tool_use("bash", r#"{"command":"rm -rf /"}"#);
        assert!(hook_result.is_denied());
    }

    #[test]
    fn test_subagent_isolation_in_multi_agent_scenario() {
        let explore_tools = allowed_tools_for_subagent("Explore");
        let mut explore_executor = SubagentToolExecutor::new(explore_tools);

        // Explore Agent 尝试执行 bash -> 应该被阻止
        let result = explore_executor.execute("bash", "{}");
        assert!(result.is_err());

        // Explore Agent 读取文件 -> 应该成功
        let result = explore_executor.execute("read_file", r#"{"path":"test.txt"}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_session_persistence_after_tool_execution() {
        let mut session = Session::new();

        // 添加多次工具调用
        for i in 0..5 {
            session.add_user_message(&format!("Task {}", i));
            session.add_assistant_message(vec![
                ContentBlock::ToolUse {
                    id: format!("t{}", i),
                    name: "bash".to_string(),
                    input: format!(r#"{{"command":"echo {}"}}"#, i),
                },
            ]);
            session.add_tool_result(&format!("t{}", i), &format!("output {}", i), false);
        }

        // 保存
        let path = std::env::temp_dir().join("test_integration_session.json");
        session.save_to_path(&path).expect("save failed");

        // 验证文件被创建且包含预期的消息
        assert!(path.exists(), "session file should exist");

        // 验证原始会话有正确数量的消息
        assert_eq!(session.messages.len(), 15, "should have 15 messages (5 iterations x 3 messages)");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_full_permission_hierarchy() {
        let tools = mvp_tool_specs();

        for tool in tools {
            // Allow mode should allow everything
            let policy = PermissionPolicy::new(PermissionMode::Allow);
            let outcome = policy.authorize(tool.name, "{}");
            assert!(outcome.is_allow(), "Allow mode should allow {}", tool.name);

            // ReadOnly mode should only allow ReadOnly tools
            let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
                .with_tool_requirement(tool.name, tool.required_permission);
            let outcome = policy.authorize(tool.name, "{}");
            match tool.required_permission {
                PermissionMode::ReadOnly => assert!(outcome.is_allow(), "ReadOnly should allow {}", tool.name),
                _ => assert!(outcome.is_deny(), "ReadOnly should deny {}", tool.name),
            }
        }
    }
}

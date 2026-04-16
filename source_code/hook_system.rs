/**
 * @file hook_system.rs
 * @brief ClaudeCode Agent Orchestrator - Hook 系统实现
 *
 * 本文件实现了工具执行生命周期的拦截机制：
 * - PreToolUse Hook: 工具执行前调用
 * - PostToolUse Hook: 工具执行后调用
 *
 * Hook 机制允许在工具执行前后进行拦截、修改和阻止操作
 *
 * @author ClaudeCode Research Team
 * @date 2026-04-14
 */

use std::ffi::OsStr;
use std::process::Command;

use serde_json::json;

// ============================================================
// Hook 相关类型定义
// ============================================================

/**
 * Hook 事件类型
 *
 * # 变体说明
 * - PreToolUse: 工具执行前触发
 * - PostToolUse: 工具执行后触发
 *
 * # 使用场景
 * - PreToolUse: 验证输入、修改参数、阻止执行
 * - PostToolUse: 验证输出、修改结果、记录日志
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
}

impl HookEvent {
    fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
        }
    }
}

/**
 * Hook 执行结果
 *
 * # 字段说明
 * - denied: 是否拒绝执行
 * - messages: Hook 返回的消息列表
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRunResult {
    denied: bool,
    messages: Vec<String>,
}

impl HookRunResult {
    /**
     * 创建允许执行的结果
     */
    #[must_use]
    pub fn allow(messages: Vec<String>) -> Self {
        Self {
            denied: false,
            messages,
        }
    }

    /**
     * 检查是否被拒绝
     */
    #[must_use]
    pub fn is_denied(&self) -> bool {
        self.denied
    }

    /**
     * 获取消息列表
     */
    #[must_use]
    pub fn messages(&self) -> &[String] {
        &self.messages
    }
}

/**
 * Hook 运行器配置
 *
 * # 字段说明
 * - config: 运行时特性配置（包含 Hook 脚本列表）
 */
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookRunner {
    config: RuntimeHookConfig,
}

/**
 * Hook 命令请求 - 传递给 Hook 脚本的上下文信息
 */
#[derive(Debug, Clone, Copy)]
struct HookCommandRequest<'a> {
    event: HookEvent,
    tool_name: &'a str,
    tool_input: &'a str,
    tool_output: Option<&'a str>,
    is_error: bool,
    payload: &'a str,
}

// ============================================================
// HookRunner 实现
// ============================================================

impl HookRunner {
    /**
     * 使用配置创建 HookRunner
     */
    #[must_use]
    pub fn new(config: RuntimeHookConfig) -> Self {
        Self { config }
    }

    /**
     * 从特性配置创建 HookRunner
     */
    #[must_use]
    pub fn from_feature_config(feature_config: &RuntimeFeatureConfig) -> Self {
        Self::new(feature_config.hooks().clone())
    }

    /**
     * 执行 PreToolUse Hook
     *
     * # 调用时机
     * 在工具执行之前调用
     *
     * # 用途
     * - 验证工具输入参数
     * - 检查执行条件
     * - 修改工具参数
     * - 阻止危险操作
     *
     * @param tool_name 工具名称
     * @param tool_input 工具输入参数
     * @return HookRunResult 执行结果
     */
    #[must_use]
    pub fn run_pre_tool_use(&self, tool_name: &str, tool_input: &str) -> HookRunResult {
        self.run_commands(
            HookEvent::PreToolUse,
            self.config.pre_tool_use(),
            tool_name,
            tool_input,
            None,
            false,
        )
    }

    /**
     * 执行 PostToolUse Hook
     *
     * # 调用时机
     * 在工具执行之后调用
     *
     * # 用途
     * - 验证工具输出结果
     * - 记录执行日志
     * - 修改工具输出
     * - 清理资源
     *
     * @param tool_name 工具名称
     * @param tool_input 工具输入参数
     * @param tool_output 工具输出结果
     * @param is_error 是否为错误结果
     * @return HookRunResult 执行结果
     */
    #[must_use]
    pub fn run_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: &str,
        tool_output: &str,
        is_error: bool,
    ) -> HookRunResult {
        self.run_commands(
            HookEvent::PostToolUse,
            self.config.post_tool_use(),
            tool_name,
            tool_input,
            Some(tool_output),
            is_error,
        )
    }

    /**
     * 执行 Hook 命令列表
     *
     * # 算法
     * 1. 构建 JSON payload
     * 2. 依次执行每个 Hook 命令
     * 3. 根据退出码决定结果：
     *    - 0: Allow - 允许执行，继续
     *    - 2: Deny - 拒绝执行，立即返回
     *    - 其他: Warn - 警告但继续
     * 4. 合并所有 Hook 的消息
     *
     * @param event Hook 事件类型
     * @param commands Hook 命令列表
     * @param tool_name 工具名称
     * @param tool_input 工具输入
     * @param tool_output 工具输出（PostToolUse 时提供）
     * @param is_error 是否为错误
     */
    fn run_commands(
        &self,
        event: HookEvent,
        commands: &[String],
        tool_name: &str,
        tool_input: &str,
        tool_output: Option<&str>,
        is_error: bool,
    ) -> HookRunResult {
        // 如果没有 Hook 命令，直接允许
        if commands.is_empty() {
            return HookRunResult::allow(Vec::new());
        }

        // 构建 JSON payload
        let payload = json!({
            "hook_event_name": event.as_str(),
            "tool_name": tool_name,
            "tool_input": parse_tool_input(tool_input),
            "tool_input_json": tool_input,
            "tool_output": tool_output,
            "tool_result_is_error": is_error,
        })
        .to_string();

        let mut messages = Vec::new();

        // 依次执行每个 Hook 命令
        for command in commands {
            match Self::run_command(
                command,
                HookCommandRequest {
                    event,
                    tool_name,
                    tool_input,
                    tool_output,
                    is_error,
                    payload: &payload,
                },
            ) {
                HookCommandOutcome::Allow { message } => {
                    // 允许执行，合并消息
                    if let Some(message) = message {
                        messages.push(message);
                    }
                }
                HookCommandOutcome::Deny { message } => {
                    // 拒绝执行，立即返回
                    let message = message.unwrap_or_else(|| {
                        format!("{} hook denied tool `{}`", event.as_str(), tool_name)
                    });
                    messages.push(message);
                    return HookRunResult {
                        denied: true,
                        messages,
                    };
                }
                HookCommandOutcome::Warn { message } => {
                    // 警告但继续，合并消息
                    messages.push(message);
                }
            }
        }

        HookRunResult::allow(messages)
    }

    /**
     * 运行单个 Hook 命令
     *
     * # 环境变量
     * Hook 脚本通过环境变量接收上下文：
     * - HOOK_EVENT: 事件类型 ("PreToolUse" 或 "PostToolUse")
     * - HOOK_TOOL_NAME: 工具名称
     * - HOOK_TOOL_INPUT: 工具输入（原始 JSON 字符串）
     * - HOOK_TOOL_OUTPUT: 工具输出（仅 PostToolUse）
     * - HOOK_TOOL_IS_ERROR: 是否为错误 ("0" 或 "1")
     *
     * # 退出码语义
     * - 0: Allow - 允许执行
     * - 2: Deny - 拒绝执行
     * - 其他: Warn - 警告但继续
     */
    fn run_command(command: &str, request: HookCommandRequest<'_>) -> HookCommandOutcome {
        let mut child = shell_command(command);

        // 配置输入输出
        child.stdin(std::process::Stdio::piped());
        child.stdout(std::process::Stdio::piped());
        child.stderr(std::process::Stdio::piped());

        // 设置环境变量
        child.env("HOOK_EVENT", request.event.as_str());
        child.env("HOOK_TOOL_NAME", request.tool_name);
        child.env("HOOK_TOOL_INPUT", request.tool_input);
        child.env(
            "HOOK_TOOL_IS_ERROR",
            if request.is_error { "1" } else { "0" },
        );

        // PostToolUse 时传递输出
        if let Some(tool_output) = request.tool_output {
            child.env("HOOK_TOOL_OUTPUT", tool_output);
        }

        // 执行命令并传递 payload 到 stdin
        match child.output_with_stdin(request.payload.as_bytes()) {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let message = (!stdout.is_empty()).then_some(stdout);

                // 根据退出码决定结果
                match output.status.code() {
                    Some(0) => HookCommandOutcome::Allow { message },
                    Some(2) => HookCommandOutcome::Deny { message },
                    Some(code) => HookCommandOutcome::Warn {
                        message: format_hook_warning(
                            command,
                            code,
                            message.as_deref(),
                            stderr.as_str(),
                        ),
                    },
                    None => HookCommandOutcome::Warn {
                        message: format!(
                            "{} hook `{}` terminated by signal while handling `{}`",
                            request.event.as_str(),
                            command,
                            request.tool_name
                        ),
                    },
                }
            }
            Err(error) => HookCommandOutcome::Warn {
                message: format!(
                    "{} hook `{}` failed to start for `{}`: {}",
                    request.event.as_str(),
                    command,
                    request.tool_name,
                    error
                ),
            },
        }
    }
}

/**
 * Hook 命令执行结果
 */
enum HookCommandOutcome {
    Allow { message: Option<String> },
    Deny { message: Option<String> },
    Warn { message: String },
}

/**
 * 解析工具输入为 JSON 值
 *
 * 如果输入不是有效的 JSON，则包装为 {"raw": input}
 */
fn parse_tool_input(tool_input: &str) -> serde_json::Value {
    serde_json::from_str(tool_input).unwrap_or_else(|_| json!({ "raw": tool_input }))
}

/**
 * 格式化 Hook 警告消息
 */
fn format_hook_warning(
    command: &str,
    code: i32,
    stdout: Option<&str>,
    stderr: &str,
) -> String {
    let mut message =
        format!("Hook `{}` exited with status {}; allowing tool execution to continue", command, code);

    if let Some(stdout) = stdout.filter(|stdout| !stdout.is_empty()) {
        message.push_str(": ");
        message.push_str(stdout);
    } else if !stderr.is_empty() {
        message.push_str(": ");
        message.push_str(stderr);
    }

    message
}

// ============================================================
// Shell 命令执行辅助
// ============================================================

/**
 * 创建 shell 命令
 *
 * # 跨平台支持
 * - Windows: 使用 cmd /C
 * - 其他: 使用 sh -lc
 */
fn shell_command(command: &str) -> CommandWithStdin {
    #[cfg(windows)]
    let mut command_builder = {
        let mut command_builder = Command::new("cmd");
        command_builder.arg("/C").arg(command);
        CommandWithStdin::new(command_builder)
    };

    #[cfg(not(windows))]
    let command_builder = {
        let mut command_builder = Command::new("sh");
        command_builder.arg("-lc").arg(command);
        CommandWithStdin::new(command_builder)
    };

    command_builder
}

/**
 * 带 stdin 的命令构建器
 *
 * # 封装目的
 * 提供 fluent API 配置命令，并通过 stdin 传递 payload
 */
struct CommandWithStdin {
    command: Command,
}

impl CommandWithStdin {
    fn new(command: Command) -> Self {
        Self { command }
    }

    fn stdin(&mut self, cfg: std::process::Stdio) -> &mut Self {
        self.command.stdin(cfg);
        self
    }

    fn stdout(&mut self, cfg: std::process::Stdio) -> &mut Self {
        self.command.stdout(cfg);
        self
    }

    fn stderr(&mut self, cfg: std::process::Stdio) -> &mut Self {
        self.command.stderr(cfg);
        self
    }

    fn env<K, V>(&mut self, key: K, value: V) -> &mut Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.command.env(key, value);
        self
    }

    /**
     * 执行命令并传递 stdin 数据
     *
     * @param stdin 要传递给 stdin 的数据
     * @return Result<Output, Error>
     */
    fn output_with_stdin(&mut self, stdin: &[u8]) -> std::io::Result<std::process::Output> {
        use std::io::Write as _;

        let mut child = self.command.spawn()?;

        // 写入 stdin
        if let Some(mut child_stdin) = child.stdin.take() {
            child_stdin.write_all(stdin)?;
        }

        child.wait_with_output()
    }
}

// ============================================================
// 测试模块
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_allows_on_exit_code_zero() {
        // 给定：退出码为 0 的 Hook
        let runner = HookRunner::new(RuntimeHookConfig::new(
            vec![shell_snippet("printf 'pre ok'")],
            Vec::new(),
        ));

        // 当：执行 PreToolUse Hook
        let result = runner.run_pre_tool_use("Read", r#"{"path":"README.md"}"#);

        // 那么：应该允许执行
        assert_eq!(result, HookRunResult::allow(vec!["pre ok".to_string()]));
        assert!(!result.is_denied());
    }

    #[test]
    fn test_hook_denies_on_exit_code_two() {
        // 给定：退出码为 2 的 Hook
        let runner = HookRunner::new(RuntimeHookConfig::new(
            vec![shell_snippet("printf 'blocked by hook'; exit 2")],
            Vec::new(),
        ));

        // 当：执行 PreToolUse Hook
        let result = runner.run_pre_tool_use("Bash", r#"{"command":"pwd"}"#);

        // 那么：应该拒绝执行
        assert!(result.is_denied());
        assert_eq!(result.messages(), &["blocked by hook".to_string()]);
    }

    #[test]
    fn test_hook_warns_on_other_exit_codes() {
        // 给定：退出码为 1 的 Hook
        let runner = HookRunner::new(RuntimeHookConfig::new(
            vec![shell_snippet("printf 'warning hook'; exit 1")],
            Vec::new(),
        ));

        // 当：执行 PreToolUse Hook
        let result = runner.run_pre_tool_use("Edit", r#"{"file":"src/lib.rs"}"#);

        // 那么：应该警告但允许执行
        assert!(!result.is_denied());
        assert!(result
            .messages()
            .iter()
            .any(|message| message.contains("allowing tool execution to continue")));
    }

    #[test]
    fn test_multiple_hooks_chain() {
        // 给定：多个 Hook 命令
        let runner = HookRunner::new(RuntimeHookConfig::new(
            vec![
                shell_snippet("printf 'hook1'"),
                shell_snippet("printf 'hook2'"),
            ],
            Vec::new(),
        ));

        // 当：执行 Hook
        let result = runner.run_pre_tool_use("Read", r#"{"path":"test.txt"}"#);

        // 那么：应该合并所有消息
        assert!(!result.is_denied());
        assert!(result.messages().contains(&"hook1".to_string()));
        assert!(result.messages().contains(&"hook2".to_string()));
    }

    #[test]
    fn test_post_tool_use_with_output() {
        // 给定：检查工具输出的 Hook
        let runner = HookRunner::new(RuntimeHookConfig::new(
            Vec::new(),
            vec![shell_snippet(
                "if [ \"$HOOK_TOOL_IS_ERROR\" = \"1\" ]; then printf 'error detected'; exit 2; fi; exit 0",
            )],
        ));

        // 当：执行 PostToolUse Hook（工具执行成功）
        let result = runner.run_post_tool_use("Bash", "{}", "output", false);

        // 那么：应该允许
        assert!(!result.is_denied());

        // 当：执行 PostToolUse Hook（工具执行失败）
        let result = runner.run_post_tool_use("Bash", "{}", "error occurred", true);

        // 那么：应该拒绝
        assert!(result.is_denied());
    }

    #[test]
    fn test_empty_commands_returns_allow() {
        // 给定：空的 Hook 命令列表
        let runner = HookRunner::new(RuntimeHookConfig::new(
            Vec::new(),
            Vec::new(),
        ));

        // 当：执行 Hook
        let result = runner.run_pre_tool_use("Read", "{}");

        // 那么：应该直接允许
        assert!(!result.is_denied());
        assert!(result.messages().is_empty());
    }

    // 跨平台 shell snippet
    #[cfg(windows)]
    fn shell_snippet(script: &str) -> String {
        script.replace('\'', "\"")
    }

    #[cfg(not(windows))]
    fn shell_snippet(script: &str) -> String {
        script.to_string()
    }
}

// ============================================================
// 使用示例
// ============================================================

/*
使用示例：配置 Hook 实现安全策略

```rust
// 创建 Hook 配置
let config = RuntimeHookConfig::new(
    // PreToolUse Hooks: 验证和修改
    vec![
        // 阻止危险的 bash 命令
        r#"if echo "$HOOK_TOOL_INPUT" | grep -q "rm -rf"; then
            printf "Dangerous command blocked";
            exit 2;
        fi"#.to_string(),

        // 记录所有文件操作
        r#"printf "File operation: $HOOK_TOOL_NAME""#.to_string(),
    ],
    // PostToolUse Hooks: 验证和记录
    vec![
        // 检查输出是否包含敏感信息
        r#"if echo "$HOOK_TOOL_OUTPUT" | grep -qi "password\|secret"; then
            printf "Warning: sensitive data in output";
            exit 1;
        fi"#.to_string(),
    ],
);

let runner = HookRunner::new(config);

// 执行 Hook
let result = runner.run_pre_tool_use(
    "bash",
    r#"{"command":"rm -rf /tmp/test"}"#,
);

if result.is_denied() {
    println!("Tool execution blocked: {}", result.messages().join(", "));
}
```
*/

// Placeholder types that would be imported from other crates
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookConfig {
    pre_tool_use: Vec<String>,
    post_tool_use: Vec<String>,
}

impl RuntimeHookConfig {
    pub fn new(pre_tool_use: Vec<String>, post_tool_use: Vec<String>) -> Self {
        Self { pre_tool_use, post_tool_use }
    }

    pub fn pre_tool_use(&self) -> &[String] {
        &self.pre_tool_use
    }

    pub fn post_tool_use(&self) -> &[String] {
        &self.post_tool_use
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeFeatureConfig {
    hooks_config: RuntimeHookConfig,
}

impl RuntimeFeatureConfig {
    pub fn hooks(&self) -> &RuntimeHookConfig {
        &self.hooks_config
    }

    pub fn with_hooks(mut self, hooks: RuntimeHookConfig) -> Self {
        self.hooks_config = hooks;
        self
    }
}

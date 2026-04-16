/**
 * @file conversation_core.rs
 * @brief ClaudeCode Agent Orchestrator 核心实现 - ConversationRuntime
 *
 * 本文件实现了 Agent 的核心执行循环，包括：
 * - 用户输入处理
 * - LLM API 调用
 * - 工具调用循环
 * - 权限检查
 * - Hook 拦截
 *
 * @author ClaudeCode Research Team
 * @date 2026-04-14
 */

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use crate::compact::{
    compact_session, estimate_session_tokens, CompactionConfig, CompactionResult,
};
use crate::config::RuntimeFeatureConfig;
use crate::hooks::{HookRunResult, HookRunner};
use crate::permissions::{PermissionOutcome, PermissionPolicy, PermissionPrompter};
use crate::session::{ContentBlock, ConversationMessage, Session};
use crate::usage::{TokenUsage, UsageTracker};

// ============================================================
// 辅助数据类型定义
// ============================================================

/**
 * API 请求结构 - 封装发送给 LLM 的请求
 *
 * # 字段说明
 * - system_prompt: 系统提示词，用于设定 Agent 行为
 * - messages: 对话历史消息列表
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequest {
    pub system_prompt: Vec<String>,
    pub messages: Vec<ConversationMessage>,
}

/**
 * Assistant 事件类型 - LLM 响应的事件流
 *
 * # 变体说明
 * - TextDelta: 文本增量输出
 * - ToolUse: 工具调用请求
 * - Usage: Token 使用量报告
 * - MessageStop: 消息结束标记
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantEvent {
    TextDelta(String),
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    Usage(TokenUsage),
    MessageStop,
}

/**
 * API 客户端 trait - 抽象 LLM 通信接口
 *
 * # 设计目的
 * 通过 trait 抽象，允许使用不同的 API 提供者
 * (Anthropic、OpenAI、本地模型等)
 */
pub trait ApiClient {
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError>;
}

/**
 * 工具执行器 trait - 抽象工具执行接口
 *
 * # 设计目的
 * 支持不同的工具执行策略：
 * - 内置工具执行
 * - 子Agent工具执行（带权限过滤）
 * - 沙箱环境工具执行
 */
pub trait ToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError>;
}

/**
 * 工具执行错误
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    message: String,
}

impl ToolError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ToolError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ToolError {}

/**
 * 运行时错误
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    message: String,
}

impl RuntimeError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

/**
 * Turn 摘要 - Agent 执行一圈的结果
 *
 * # 字段说明
 * - assistant_messages: Assistant 发送的消息列表
 * - tool_results: 工具执行结果列表
 * - iterations: 执行的迭代次数
 * - usage: Token 使用量统计
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSummary {
    pub assistant_messages: Vec<ConversationMessage>,
    pub tool_results: Vec<ConversationMessage>,
    pub iterations: usize,
    pub usage: TokenUsage,
}

// ============================================================
// ConversationRuntime 核心实现
// ============================================================

/**
 * ConversationRuntime - Agent 的核心执行引擎
 *
 * # 类型参数
 * - C: ApiClient - LLM API 客户端实现
 * - T: ToolExecutor - 工具执行器实现
 *
 * # 设计特点
 * 1. 泛型设计：支持多种 API 提供者和工具执行策略
 * 2. 策略模式：权限策略和 Hook 运行器可独立配置
 * 3. 循环不变式：每次循环推进对话状态，直到任务完成或达到限制
 *
 * # 安全机制
 * 1. 权限检查：每次工具执行前进行权限验证
 * 2. Hook 拦截：Pre/Post 工具 Hook 可以修改或阻止执行
 * 3. 迭代限制：max_iterations 防止无限循环
 */
pub struct ConversationRuntime<C, T> {
    session: Session,                      // 对话历史状态
    api_client: C,                        // LLM API 客户端
    tool_executor: T,                     // 工具执行器
    permission_policy: PermissionPolicy,   // 权限策略
    system_prompt: Vec<String>,            // 系统提示词
    max_iterations: usize,                // 最大迭代次数（安全保护）
    usage_tracker: UsageTracker,          // Token 使用量追踪
    hook_runner: HookRunner,              // Hook 运行器
}

impl<C, T> ConversationRuntime<C, T>
where
    C: ApiClient,
    T: ToolExecutor,
{
    /**
     * 创建新的 ConversationRuntime 实例
     *
     * # 参数
     * - session: 初始会话状态
     * - api_client: API 客户端实例
     * - tool_executor: 工具执行器实例
     * - permission_policy: 权限策略
     * - system_prompt: 系统提示词
     */
    #[must_use]
    pub fn new(
        session: Session,
        api_client: C,
        tool_executor: T,
        permission_policy: PermissionPolicy,
        system_prompt: Vec<String>,
    ) -> Self {
        Self::new_with_features(
            session,
            api_client,
            tool_executor,
            permission_policy,
            system_prompt,
            RuntimeFeatureConfig::default(),
        )
    }

    /**
     * 使用特性配置创建 ConversationRuntime
     */
    #[must_use]
    pub fn new_with_features(
        session: Session,
        api_client: C,
        tool_executor: T,
        permission_policy: PermissionPolicy,
        system_prompt: Vec<String>,
        feature_config: RuntimeFeatureConfig,
    ) -> Self {
        let usage_tracker = UsageTracker::from_session(&session);
        Self {
            session,
            api_client,
            tool_executor,
            permission_policy,
            system_prompt,
            max_iterations: usize::MAX,
            usage_tracker,
            hook_runner: HookRunner::from_feature_config(&feature_config),
        }
    }

    /**
     * 设置最大迭代次数
     *
     * # 用途
     * 防止 Agent 进入无限循环，提供安全保护
     */
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /**
     * run_turn - Agent 执行一圈的核心方法
     *
     * # 算法流程
     * ```
     * 1. 添加用户消息到会话
     * 2. 循环直到没有待执行工具:
     *    a. 调用 LLM API 获取响应
     *    b. 解析响应事件流
     *    c. 提取待执行的工具调用
     *    d. 对每个工具:
     *       i.   检查权限
     *       ii.  执行 PreToolUse Hook
     *       iii. 执行工具
     *       iv.  执行 PostToolUse Hook
     *       v.   将结果添加到会话
     * 3. 返回 TurnSummary
     * ```
     *
     * # 安全保护
     * - 迭代次数限制：超过 max_iterations 返回错误
     * - 权限检查：每个工具执行前验证权限
     * - Hook 拦截：可在执行前/后拦截和修改
     */
    pub fn run_turn(
        &mut self,
        user_input: impl Into<String>,
        mut prompter: Option<&mut dyn PermissionPrompter>,
    ) -> Result<TurnSummary, RuntimeError> {
        // Step 1: 将用户输入作为消息添加到会话历史
        self.session
            .messages
            .push(ConversationMessage::user_text(user_input.into()));

        let mut assistant_messages = Vec::new();
        let mut tool_results = Vec::new();
        let mut iterations = 0;

        // Step 2: 进入 Agent 循环 - 持续执行工具调用直到完成
        loop {
            iterations += 1;

            // 安全检查：防止无限循环
            if iterations > self.max_iterations {
                return Err(RuntimeError::new(
                    "conversation loop exceeded the maximum number of iterations",
                ));
            }

            // Step 2a: 构建 API 请求
            let request = ApiRequest {
                system_prompt: self.system_prompt.clone(),
                messages: self.session.messages.clone(),
            };

            // Step 2b: 调用 LLM API 获取响应
            let events = self.api_client.stream(request)?;

            // Step 2c: 从事件流构建 Assistant 消息
            let (assistant_message, usage) = build_assistant_message(events)?;

            // 记录 Token 使用量
            if let Some(usage) = usage {
                self.usage_tracker.record(usage);
            }

            // Step 2d: 提取所有待执行的工具调用
            let pending_tool_uses = assistant_message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse { id, name, input } => {
                        Some((id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            // 将 Assistant 消息添加到会话历史
            self.session.messages.push(assistant_message.clone());
            assistant_messages.push(assistant_message);

            // 如果没有待执行工具，说明这轮对话已完成
            if pending_tool_uses.is_empty() {
                break;
            }

            // Step 2e: 执行每个待处理的工具调用
            for (tool_use_id, tool_name, input) in pending_tool_uses {
                // Step 2e.i: 权限检查
                let permission_outcome = if let Some(prompt) = prompter.as_mut() {
                    self.permission_policy
                        .authorize(&tool_name, &input, Some(*prompt))
                } else {
                    self.permission_policy.authorize(&tool_name, &input, None)
                };

                let result_message = match permission_outcome {
                    PermissionOutcome::Allow => {
                        // 执行 PreToolUse Hook
                        let pre_hook_result =
                            self.hook_runner.run_pre_tool_use(&tool_name, &input);

                        if pre_hook_result.is_denied() {
                            // Hook 拒绝执行工具
                            let deny_message = format!(
                                "PreToolUse hook denied tool `{}`",
                                tool_name
                            );
                            ConversationMessage::tool_result(
                                tool_use_id,
                                tool_name,
                                format_hook_message(&pre_hook_result, &deny_message),
                                true,
                            )
                        } else {
                            // 执行工具
                            let (mut output, mut is_error) =
                                match self.tool_executor.execute(&tool_name, &input) {
                                    Ok(output) => (output, false),
                                    Err(error) => (error.to_string(), true),
                                };

                            // 合并 Pre Hook 的反馈消息到输出
                            output =
                                merge_hook_feedback(pre_hook_result.messages(), output, false);

                            // 执行 PostToolUse Hook
                            let post_hook_result = self
                                .hook_runner
                                .run_post_tool_use(&tool_name, &input, &output, is_error);

                            // 检查 Post Hook 是否标记为错误
                            if post_hook_result.is_denied() {
                                is_error = true;
                            }

                            // 合并 Post Hook 的反馈消息到输出
                            output = merge_hook_feedback(
                                post_hook_result.messages(),
                                output,
                                post_hook_result.is_denied(),
                            );

                            ConversationMessage::tool_result(
                                tool_use_id,
                                tool_name,
                                output,
                                is_error,
                            )
                        }
                    }
                    PermissionOutcome::Deny { reason } => {
                        // 权限策略拒绝执行工具
                        ConversationMessage::tool_result(
                            tool_use_id,
                            tool_name,
                            reason,
                            true,
                        )
                    }
                };

                // 将工具结果添加到会话历史
                self.session.messages.push(result_message.clone());
                tool_results.push(result_message);
            }
        }

        Ok(TurnSummary {
            assistant_messages,
            tool_results,
            iterations,
            usage: self.usage_tracker.cumulative_usage(),
        })
    }

    /**
     * 压缩会话 - 减少历史消息长度
     *
     * # 用途
     * 当对话历史过长时，压缩历史以节省 Token 成本
     */
    #[must_use]
    pub fn compact(&self, config: CompactionConfig) -> CompactionResult {
        compact_session(&self.session, config)
    }

    /**
     * 估算当前会话的 Token 数量
     */
    #[must_use]
    pub fn estimated_tokens(&self) -> usize {
        estimate_session_tokens(&self.session)
    }

    /**
     * 获取 Token 使用量追踪器
     */
    #[must_use]
    pub fn usage(&self) -> &UsageTracker {
        &self.usage_tracker
    }

    /**
     * 获取会话引用
     */
    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    /**
     * 消费自身并返回内部会话
     */
    #[must_use]
    pub fn into_session(self) -> Session {
        self.session
    }
}

// ============================================================
// 辅助函数
// ============================================================

/**
 * 从事件流构建 Assistant 消息
 *
 * # 算法
 * 1. 遍历所有事件
 * 2. 收集文本增量到 text 变量
 * 3. 遇到 ToolUse 块时，先将累积的文本作为 Text 块
 * 4. 遇到 Usage 事件记录使用量
 * 5. 遇到 MessageStop 事件标记完成
 * 6. 刷新剩余的文本作为最后一个 Text 块
 */
fn build_assistant_message(
    events: Vec<AssistantEvent>,
) -> Result<(ConversationMessage, Option<TokenUsage>), RuntimeError> {
    let mut text = String::new();
    let mut blocks = Vec::new();
    let mut finished = false;
    let mut usage = None;

    for event in events {
        match event {
            AssistantEvent::TextDelta(delta) => {
                text.push_str(&delta);
            }
            AssistantEvent::ToolUse { id, name, input } => {
                // 将累积的文本作为 Text 块
                flush_text_block(&mut text, &mut blocks);
                blocks.push(ContentBlock::ToolUse { id, name, input });
            }
            AssistantEvent::Usage(value) => {
                usage = Some(value);
            }
            AssistantEvent::MessageStop => {
                finished = true;
            }
        }
    }

    // 刷新剩余的文本
    flush_text_block(&mut text, &mut blocks);

    // 验证消息完整性
    if !finished {
        return Err(RuntimeError::new(
            "assistant stream ended without a message stop event",
        ));
    }
    if blocks.is_empty() {
        return Err(RuntimeError::new(
            "assistant stream produced no content",
        ));
    }

    Ok((
        ConversationMessage::assistant_with_usage(blocks, usage),
        usage,
    ))
}

/**
 * 将累积的文本刷新为一个 Text 块
 */
fn flush_text_block(text: &mut String, blocks: &mut Vec<ContentBlock>) {
    if !text.is_empty() {
        blocks.push(ContentBlock::Text {
            text: std::mem::take(text),
        });
    }
}

/**
 * 格式化 Hook 拒绝消息
 */
fn format_hook_message(result: &HookRunResult, fallback: &str) -> String {
    if result.messages().is_empty() {
        fallback.to_string()
    } else {
        result.messages().join("\n")
    }
}

/**
 * 合并 Hook 反馈消息到工具输出
 *
 * # 逻辑
 * 如果有 Hook 消息，将其附加到输出中
 * 如果工具执行被拒绝，在输出中标记
 */
fn merge_hook_feedback(messages: &[String], output: String, denied: bool) -> String {
    if messages.is_empty() {
        return output;
    }

    let mut sections = Vec::new();
    if !output.trim().is_empty() {
        sections.push(output);
    }

    let label = if denied {
        "Hook feedback (denied)"
    } else {
        "Hook feedback"
    };
    sections.push(format!("{label}:\n{}", messages.join("\n")));
    sections.join("\n\n")
}

// ============================================================
// 静态工具执行器实现
// ============================================================

/**
 * 静态工具执行器 - 使用函数闭包注册工具处理器
 *
 * # 用途
 * 适用于工具集合固定的场景
 */
type ToolHandler = Box<dyn FnMut(&str) -> Result<String, ToolError>>;

/**
 * 静态工具执行器
 *
 * # 使用示例
 * ```rust
 * let executor = StaticToolExecutor::new()
 *     .register("bash", |input| {
 *         // 解析 input 并执行 bash 命令
 *         Ok("command output".to_string())
 *     })
 *     .register("read", |input| {
 *         // 解析 input 并读取文件
 *         Ok("file contents".to_string())
 *     });
 * ```
 */
#[derive(Default)]
pub struct StaticToolExecutor {
    handlers: BTreeMap<String, ToolHandler>,
}

impl StaticToolExecutor {
    /**
     * 创建新的空执行器
     */
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /**
     * 注册一个工具处理器
     *
     * # 参数
     * - tool_name: 工具名称
     * - handler: 处理函数闭包
     *
     * # 返回
     * 返回 self 以支持链式调用
     */
    #[must_use]
    pub fn register(
        mut self,
        tool_name: impl Into<String>,
        handler: impl FnMut(&str) -> Result<String, ToolError> + 'static,
    ) -> Self {
        self.handlers.insert(tool_name.into(), Box::new(handler));
        self
    }
}

impl ToolExecutor for StaticToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        self.handlers
            .get_mut(tool_name)
            .ok_or_else(|| ToolError::new(format!("unknown tool: {tool_name}")))?(input)
    }
}

// ============================================================
// 测试模块
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // 测试辅助：模拟 API 客户端
    struct MockApiClient {
        responses: Vec<Vec<AssistantEvent>>,
        call_count: usize,
    }

    impl MockApiClient {
        fn new(responses: Vec<Vec<AssistantEvent>>) -> Self {
            Self {
                responses,
                call_count: 0,
            }
        }
    }

    impl ApiClient for MockApiClient {
        fn stream(&mut self, _request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
            if self.call_count < self.responses.len() {
                let response = self.responses[self.call_count].clone();
                self.call_count += 1;
                Ok(response)
            } else {
                Err(RuntimeError::new("no more responses"))
            }
        }
    }

    #[test]
    fn test_single_turn_no_tools() {
        // 测试：单轮对话，无工具调用
        let api_client = MockApiClient::new(vec![vec![
            AssistantEvent::TextDelta("Hello, how can I help?".to_string()),
            AssistantEvent::Usage(TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
            }),
            AssistantEvent::MessageStop,
        ]]);

        let tool_executor = StaticToolExecutor::new();
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            api_client,
            tool_executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["You are a helpful assistant.".to_string()],
        );

        let result = runtime.run_turn("Hi!", None).unwrap();

        assert_eq!(result.iterations, 1);
        assert_eq!(result.assistant_messages.len(), 1);
        assert_eq!(result.tool_results.len(), 0);
    }

    #[test]
    fn test_tool_execution_flow() {
        // 测试：工具执行流程
        let api_client = MockApiClient::new(vec![
            // 第一轮：返回工具调用
            vec![
                AssistantEvent::TextDelta("Let me check that file.".to_string()),
                AssistantEvent::ToolUse {
                    id: "tool-1".to_string(),
                    name: "read_file".to_string(),
                    input: r#"{"path":"test.txt"}"#.to_string(),
                },
                AssistantEvent::Usage(TokenUsage {
                    input_tokens: 20,
                    output_tokens: 10,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                }),
                AssistantEvent::MessageStop,
            ],
            // 第二轮：返回最终响应
            vec![
                AssistantEvent::TextDelta("The file contains: hello world".to_string()),
                AssistantEvent::MessageStop,
            ],
        ]);

        let tool_executor = StaticToolExecutor::new()
            .register("read_file", |input| {
                Ok("file contents: hello world".to_string())
            });

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            api_client,
            tool_executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["You are a helpful assistant.".to_string()],
        );

        let result = runtime.run_turn("Read test.txt", None).unwrap();

        assert_eq!(result.iterations, 2);
        assert_eq!(result.assistant_messages.len(), 2);
        assert_eq!(result.tool_results.len(), 1);
    }

    #[test]
    fn test_max_iterations_protection() {
        // 测试：最大迭代次数保护
        let api_client = MockApiClient::new(vec![
            vec![
                AssistantEvent::ToolUse {
                    id: "tool-1".to_string(),
                    name: "infinite".to_string(),
                    input: "{}".to_string(),
                },
                AssistantEvent::MessageStop,
            ];
            100 // 100 个响应，但 max_iterations = 2
        ]);

        let tool_executor = StaticToolExecutor::new()
            .register("infinite", |_| Ok("done".to_string()));

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            api_client,
            tool_executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["You are a helpful assistant.".to_string()],
        )
        .with_max_iterations(2);

        let result = runtime.run_turn("trigger infinite loop", None);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max iterations"));
    }
}

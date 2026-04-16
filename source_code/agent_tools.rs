/**
 * @file agent_tools.rs
 * @brief ClaudeCode Agent Orchestrator - 子Agent与工具执行实现
 *
 * 本文件实现了：
 * - 子Agent的创建、执行、隔离
 * - 工具白名单机制
 * - Agent注册表和工具规范定义
 *
 * @author ClaudeCode Research Team
 * @date 2026-04-14
 */

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

// ============================================================
// 工具相关类型定义
// ============================================================

/**
 * 工具清单条目 - 记录可用工具的元信息
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolManifestEntry {
    pub name: String,
    pub source: ToolSource,
}

/**
 * 工具来源类型
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    Base,       // 内置工具
    Conditional, // 条件工具
}

/**
 * 工具注册表
 */
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolRegistry {
    entries: Vec<ToolManifestEntry>,
}

impl ToolRegistry {
    pub fn new(entries: Vec<ToolManifestEntry>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[ToolManifestEntry] {
        &self.entries
    }
}

/**
 * 工具规范 - 定义单个工具的元信息
 *
 * # 字段说明
 * - name: 工具名称
 * - description: 工具描述（用于 LLM 理解工具用途）
 * - input_schema: JSON Schema 格式的输入规范
 * - required_permission: 执行该工具所需的最低权限
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: serde_json::Value,
    pub required_permission: PermissionMode,
}

// ============================================================
// 子Agent相关类型定义
// ============================================================

/**
 * Agent 输入 - 创建子Agent的参数
 *
 * # 字段说明
 * - description: Agent 描述，用于标识和日志
 * - prompt: 任务提示词
 * - subagent_type: 子Agent类型（决定可用工具集）
 * - name: 可选的 Agent 名称
 * - model: 可选的模型名称
 */
#[derive(Debug, Clone)]
pub struct AgentInput {
    pub description: String,
    pub prompt: String,
    pub subagent_type: Option<String>,
    pub name: Option<String>,
    pub model: Option<String>,
}

/**
 * Agent 输出 - 存储Agent执行结果和元数据
 *
 * # 状态流转
 * - created: 创建清单文件
 * - running: 线程已启动
 * - completed: 任务成功完成
 * - failed: 任务执行失败
 */
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentOutput {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub subagent_type: Option<String>,
    pub model: Option<String>,
    pub status: String,           // "running" | "completed" | "failed"
    pub output_file: String,      // 输出文件路径
    pub manifest_file: String,    // 清单文件路径
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<String>,
}

/**
 * AgentJob - 在线程中执行的Agent任务
 *
 * # 设计目的
 * 封装在独立线程中执行所需的所有数据
 */
#[derive(Debug, Clone)]
struct AgentJob {
    manifest: AgentOutput,
    prompt: String,
    system_prompt: Vec<String>,
    allowed_tools: BTreeSet<String>,
}

// ============================================================
// 常量定义
// ============================================================

/// 默认Agent模型
const DEFAULT_AGENT_MODEL: &str = "claude-opus-4-6";

/// 默认Agent系统日期
const DEFAULT_AGENT_SYSTEM_DATE: &str = "2026-03-31";

/// 默认最大迭代次数
const DEFAULT_AGENT_MAX_ITERATIONS: usize = 32;

// ============================================================
// 子Agent执行核心函数
// ============================================================

/**
 * execute_agent - 启动子Agent的主入口函数
 *
 * # 算法流程
 * ```
 * 1. 验证输入参数（非空检查）
 * 2. 生成唯一 agent_id
 * 3. 创建输出目录
 * 4. 解析子Agent类型和模型
 * 5. 构建系统提示词
 * 6. 根据类型确定允许的工具集
 * 7. 创建并持久化清单文件
 * 8. 在新线程中启动执行
 * 9. 返回 AgentOutput（不等待完成）
 * ```
 *
 * # 线程模型
 * 子Agent在独立线程中执行，主线程立即返回
 * 这允许主Agent并行启动多个子Agent
 *
 * @param input Agent 输入参数
 * @return Result<AgentOutput, String> 成功返回Agent元数据，失败返回错误
 */
fn execute_agent(input: AgentInput) -> Result<AgentOutput, String> {
    execute_agent_with_spawn(input, spawn_agent_job)
}

/**
 * execute_agent_with_spawn - 带有自定义 spawn 函数的执行入口
 *
 * # 用途
 * 允许注入自定义的线程创建逻辑（用于测试）
 */
fn execute_agent_with_spawn<F>(input: AgentInput, spawn_fn: F) -> Result<AgentOutput, String>
where
    F: FnOnce(AgentJob) -> Result<(), String>,
{
    // Step 1: 输入验证
    if input.description.trim().is_empty() {
        return Err(String::from("description must not be empty"));
    }
    if input.prompt.trim().is_empty() {
        return Err(String::from("prompt must not be empty"));
    }

    // Step 2: 生成唯一 ID
    let agent_id = make_agent_id();

    // Step 3: 创建输出目录
    let output_dir = agent_store_dir()?;
    std::fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;

    // Step 4: 准备文件路径
    let output_file = output_dir.join(format!("{agent_id}.md"));
    let manifest_file = output_dir.join(format!("{agent_id}.json"));

    // Step 5: 解析子Agent类型和模型
    let normalized_subagent_type = normalize_subagent_type(input.subagent_type.as_deref());
    let model = resolve_agent_model(input.model.as_deref());

    // Step 6: 生成Agent名称
    let agent_name = input
        .name
        .as_deref()
        .map(slugify_agent_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| slugify_agent_name(&input.description));

    // Step 7: 获取创建时间
    let created_at = iso8601_now();

    // Step 8: 构建系统提示词
    let system_prompt = build_agent_system_prompt(&normalized_subagent_type)?;

    // Step 9: 根据类型确定允许的工具集
    let allowed_tools = allowed_tools_for_subagent(&normalized_subagent_type);

    // Step 10: 创建输出文件内容
    let output_contents = format!(
        "# Agent Task\n\n\
         - id: {}\n\
         - name: {}\n\
         - description: {}\n\
         - subagent_type: {}\n\
         - created_at: {}\n\n\
         ## Prompt\n\n\
         {}\n",
        agent_id,
        agent_name,
        input.description,
        normalized_subagent_type,
        created_at,
        input.prompt
    );
    std::fs::write(&output_file, output_contents).map_err(|error| error.to_string())?;

    // Step 11: 创建清单
    let manifest = AgentOutput {
        agent_id,
        name: agent_name,
        description: input.description,
        subagent_type: Some(normalized_subagent_type),
        model: Some(model),
        status: String::from("running"),
        output_file: output_file.display().to_string(),
        manifest_file: manifest_file.display().to_string(),
        created_at: created_at.clone(),
        started_at: Some(created_at),
        completed_at: None,
        error: None,
    };

    // Step 12: 持久化清单文件
    write_agent_manifest(&manifest)?;

    // Step 13: 创建 Job 并启动线程
    let manifest_for_spawn = manifest.clone();
    let job = AgentJob {
        manifest: manifest_for_spawn,
        prompt: input.prompt,
        system_prompt,
        allowed_tools,
    };

    // 执行 spawn，如果失败则更新清单为 failed 状态
    if let Err(error) = spawn_fn(job) {
        let error = format!("failed to spawn sub-agent: {error}");
        persist_agent_terminal_state(&manifest, "failed", None, Some(error.clone()))?;
        return Err(error);
    }

    Ok(manifest)
}

/**
 * spawn_agent_job - 在独立线程中启动Agent任务
 *
 * # 线程模型
 * - 每个子Agent拥有自己的线程
 * - 线程名称格式：claw-agent-{agent_id}
 * - 使用 panic 捕获防止线程崩溃影响主程序
 *
 * @param job 要执行的Agent任务
 * @return Result<(), String> 成功返回空，失败返回错误
 */
fn spawn_agent_job(job: AgentJob) -> Result<(), String> {
    let thread_name = format!("claw-agent-{}", job.manifest.agent_id);

    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            // 捕获 panic，防止线程崩溃
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_agent_job(&job)));

            match result {
                Ok(Ok(())) => {
                    // 正常完成
                }
                Ok(Err(error)) => {
                    // 执行错误
                    let _ = persist_agent_terminal_state(
                        &job.manifest,
                        "failed",
                        None,
                        Some(error),
                    );
                }
                Err(_) => {
                    // 线程 panic
                    let _ = persist_agent_terminal_state(
                        &job.manifest,
                        "failed",
                        None,
                        Some(String::from("sub-agent thread panicked")),
                    );
                }
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/**
 * run_agent_job - 在线程中执行单个Agent任务
 *
 * # 流程
 * 1. 构建Agent运行时
 * 2. 执行任务（单轮）
 * 3. 持久化结果
 */
fn run_agent_job(job: &AgentJob) -> Result<(), String> {
    // 构建运行时
    let mut runtime = build_agent_runtime(job)?.with_max_iterations(DEFAULT_AGENT_MAX_ITERATIONS);

    // 执行任务
    let summary = runtime
        .run_turn(job.prompt.clone(), None)
        .map_err(|error| error.to_string())?;

    // 提取最终文本
    let final_text = final_assistant_text(&summary);

    // 持久化结果
    persist_agent_terminal_state(
        &job.manifest,
        "completed",
        Some(final_text.as_str()),
        None,
    )
}

/**
 * build_agent_runtime - 为子Agent构建运行时
 *
 * # 设计
 * 为每个子Agent创建独立的 ConversationRuntime 实例
 * - API 客户端：使用指定的模型
 * - 工具执行器：使用白名单过滤
 * - 权限策略：使用完整权限
 */
fn build_agent_runtime(
    job: &AgentJob,
) -> Result<ConversationRuntime<ProviderRuntimeClient, SubagentToolExecutor>, String> {
    let model = job
        .manifest
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_AGENT_MODEL.to_string());

    let allowed_tools = job.allowed_tools.clone();

    // 创建 API 客户端
    let api_client = ProviderRuntimeClient::new(model, allowed_tools.clone())?;

    // 创建工具执行器（带权限过滤）
    let tool_executor = SubagentToolExecutor::new(allowed_tools);

    Ok(ConversationRuntime::new(
        Session::new(),
        api_client,
        tool_executor,
        agent_permission_policy(),
        job.system_prompt.clone(),
    ))
}

/**
 * build_agent_system_prompt - 构建Agent的系统提示词
 */
fn build_agent_system_prompt(subagent_type: &str) -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;

    let mut prompt = load_system_prompt(
        cwd,
        DEFAULT_AGENT_SYSTEM_DATE.to_string(),
        std::env::consts::OS,
        "unknown",
    )
    .map_err(|error| error.to_string())?;

    // 添加Agent类型特定的指令
    prompt.push(format!(
        "You are a background sub-agent of type `{}`. Work only on the delegated task, \
         use only the tools available to you, do not ask the user questions, \
         and finish with a concise result.",
        subagent_type
    ));

    Ok(prompt)
}

// ============================================================
// 工具白名单机制
// ============================================================

/**
 * allowed_tools_for_subagent - 根据子Agent类型确定允许的工具集
 *
 * # 设计理念
 * 不同类型的子Agent有不同的工具权限：
 * - Explore: 只读工具（代码探索）
 * - Plan: 只读 + 任务写入
 * - Verification: 包含bash执行（测试验证）
 * - 默认: 完整权限
 *
 * # 安全性
 * 通过白名单机制，即使子Agent被恶意提示词注入，
 * 也无法执行 allowed_tools 之外的工具
 *
 * @param subagent_type 子Agent类型
 * @return BTreeSet<String> 允许的工具名称集合
 */
fn allowed_tools_for_subagent(subagent_type: &str) -> BTreeSet<String> {
    let tools = match subagent_type {
        // Explore 类型 - 代码探索专用，只读工具
        "Explore" => vec![
            "read_file",
            "glob_search",
            "grep_search",
            "WebFetch",
            "WebSearch",
            "ToolSearch",
            "Skill",
            "StructuredOutput",
        ],

        // Plan 类型 - 任务规划，包含任务写入
        "Plan" => vec![
            "read_file",
            "glob_search",
            "grep_search",
            "WebFetch",
            "WebSearch",
            "ToolSearch",
            "Skill",
            "TodoWrite",
            "StructuredOutput",
            "SendUserMessage",
        ],

        // Verification 类型 - 测试验证，包含bash执行
        "Verification" => vec![
            "bash",
            "read_file",
            "glob_search",
            "grep_search",
            "WebFetch",
            "WebSearch",
            "ToolSearch",
            "TodoWrite",
            "StructuredOutput",
            "SendUserMessage",
            "PowerShell",
        ],

        // claw-guide 类型 - 指南工具
        "claw-guide" => vec![
            "read_file",
            "glob_search",
            "grep_search",
            "WebFetch",
            "WebSearch",
            "ToolSearch",
            "Skill",
            "StructuredOutput",
            "SendUserMessage",
        ],

        // statusline-setup 类型 - 状态栏设置
        "statusline-setup" => vec![
            "bash",
            "read_file",
            "write_file",
            "edit_file",
            "glob_search",
            "grep_search",
            "ToolSearch",
        ],

        // 默认类型 - 完整权限
        _ => vec![
            "bash",
            "read_file",
            "write_file",
            "edit_file",
            "glob_search",
            "grep_search",
            "WebFetch",
            "WebSearch",
            "TodoWrite",
            "Skill",
            "ToolSearch",
            "NotebookEdit",
            "Sleep",
            "SendUserMessage",
            "Config",
            "StructuredOutput",
            "REPL",
            "PowerShell",
        ],
    };

    tools.into_iter().map(str::to_string).collect()
}

// ============================================================
// SubagentToolExecutor - 子Agent工具执行器
// ============================================================

/**
 * SubagentToolExecutor - 子Agent的工具执行器
 *
 * # 核心功能
 * 1. 白名单过滤：只允许执行 allowed_tools 中的工具
 * 2. 参数验证：解析 JSON 输入，确保格式正确
 * 3. 错误转换：将执行错误转换为 ToolError
 *
 * # 安全性
 * 即使子Agent被恶意提示词注入，也无法执行 allowed_tools 之外的工具
 * 这实现了不同子Agent之间的工具权限隔离
 */
struct SubagentToolExecutor {
    allowed_tools: BTreeSet<String>,
}

impl SubagentToolExecutor {
    fn new(allowed_tools: BTreeSet<String>) -> Self {
        Self { allowed_tools }
    }
}

impl ToolExecutor for SubagentToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        // Step 1: 权限检查 - 验证工具是否在白名单中
        if !self.allowed_tools.contains(tool_name) {
            return Err(ToolError::new(format!(
                "tool `{}` is not enabled for this sub-agent",
                tool_name
            )));
        }

        // Step 2: 解析输入 JSON
        let value: serde_json::Value = serde_json::from_str(input)
            .map_err(|error| ToolError::new(format!("invalid tool input JSON: {}", error)))?;

        // Step 3: 执行工具
        execute_tool(tool_name, &value).map_err(ToolError::new)
    }
}

// ============================================================
// MVP 工具列表
// ============================================================

/**
 * mvp_tool_specs - 返回最小可用工具集
 *
 * # 工具分类
 * 1. 系统工具: bash, PowerShell
 * 2. 文件工具: read_file, write_file, edit_file
 * 3. 搜索工具: glob_search, grep_search
 * 4. Web工具: WebFetch, WebSearch
 * 5. 任务工具: TodoWrite, Skill
 * 6. 通信工具: SendUserMessage
 * 7. Agent工具: Agent（启动子Agent）
 *
 * # 权限分级
 * - DangerFullAccess: bash, Agent
 * - WorkspaceWrite: write_file, edit_file, TodoWrite, NotebookEdit, Config
 * - ReadOnly: read_file, glob_search, grep_search, WebFetch, WebSearch, Skill, etc.
 */
#[must_use]
pub fn mvp_tool_specs() -> Vec<ToolSpec> {
    vec![
        // ========== 系统工具 ==========
        ToolSpec {
            name: "bash",
            description: "Execute a shell command in the current workspace.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout": { "type": "integer", "minimum": 1 },
                    "description": { "type": "string" },
                    "run_in_background": { "type": "boolean" },
                    "dangerouslyDisableSandbox": { "type": "boolean" }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::DangerFullAccess,
        },

        // ========== 文件读取工具 ==========
        ToolSpec {
            name: "read_file",
            description: "Read a text file from the workspace.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0 },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },

        // ========== 文件写入工具 ==========
        ToolSpec {
            name: "write_file",
            description: "Write a text file in the workspace.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },

        ToolSpec {
            name: "edit_file",
            description: "Replace text in a workspace file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean" }
                },
                "required": ["path", "old_string", "new_string"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },

        // ========== 搜索工具 ==========
        ToolSpec {
            name: "glob_search",
            description: "Find files by glob pattern.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },

        ToolSpec {
            name: "grep_search",
            description: "Search file contents with a regex pattern.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "glob": { "type": "string" },
                    "output_mode": { "type": "string" },
                    "-B": { "type": "integer", "minimum": 0 },
                    "-A": { "type": "integer", "minimum": 0 },
                    "-C": { "type": "integer", "minimum": 0 },
                    "context": { "type": "integer", "minimum": 0 },
                    "-n": { "type": "boolean" },
                    "-i": { "type": "boolean" },
                    "type": { "type": "string" },
                    "head_limit": { "type": "integer", "minimum": 1 },
                    "offset": { "type": "integer", "minimum": 0 },
                    "multiline": { "type": "boolean" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },

        // ========== Web工具 ==========
        ToolSpec {
            name: "WebFetch",
            description: "Fetch a URL, convert it into readable text, and answer a prompt about it.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "format": "uri" },
                    "prompt": { "type": "string" }
                },
                "required": ["url", "prompt"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },

        ToolSpec {
            name: "WebSearch",
            description: "Search the web for current information and return cited results.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 2 },
                    "allowed_domains": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "blocked_domains": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },

        // ========== 任务工具 ==========
        ToolSpec {
            name: "TodoWrite",
            description: "Update the structured task list for the current session.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "activeForm": { "type": "string" },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"]
                                }
                            },
                            "required": ["content", "activeForm", "status"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["todos"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::WorkspaceWrite,
        },

        // ========== Agent工具 ==========
        ToolSpec {
            name: "Agent",
            description: "Launch a specialized agent task and persist its handoff metadata.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "description": { "type": "string" },
                    "prompt": { "type": "string" },
                    "subagent_type": { "type": "string" },
                    "name": { "type": "string" },
                    "model": { "type": "string" }
                },
                "required": ["description", "prompt"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::DangerFullAccess,
        },

        // ========== 其他工具 ==========
        ToolSpec {
            name: "Skill",
            description: "Load a local skill definition and its instructions.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill": { "type": "string" },
                    "args": { "type": "string" }
                },
                "required": ["skill"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },

        ToolSpec {
            name: "ToolSearch",
            description: "Search for deferred or specialized tools by exact name or keywords.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },

        ToolSpec {
            name: "SendUserMessage",
            description: "Send a message to the user.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "attachments": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "status": {
                        "type": "string",
                        "enum": ["normal", "proactive"]
                    }
                },
                "required": ["message", "status"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },

        ToolSpec {
            name: "StructuredOutput",
            description: "Return structured output in the requested format.",
            input_schema: json!({
                "type": "object",
                "additionalProperties": true
            }),
            required_permission: PermissionMode::ReadOnly,
        },
    ]
}

// ============================================================
// 辅助函数
// ============================================================

/**
 * agent_permission_policy - 创建Agent的权限策略
 *
 * 使用 DangerFullAccess 作为基础权限，但根据工具规范调整
 */
fn agent_permission_policy() -> PermissionPolicy {
    mvp_tool_specs().into_iter().fold(
        PermissionPolicy::new(PermissionMode::DangerFullAccess),
        |policy, spec| policy.with_tool_requirement(spec.name, spec.required_permission),
    )
}

/**
 * write_agent_manifest - 持久化Agent清单到文件
 */
fn write_agent_manifest(manifest: &AgentOutput) -> Result<(), String> {
    std::fs::write(
        &manifest.manifest_file,
        serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

/**
 * persist_agent_terminal_state - 更新Agent终端状态
 *
 * # 状态流转
 * 1. 追加输出到输出文件
 * 2. 更新清单文件的状态字段
 */
fn persist_agent_terminal_state(
    manifest: &AgentOutput,
    status: &str,
    result: Option<&str>,
    error: Option<String>,
) -> Result<(), String> {
    append_agent_output(
        &manifest.output_file,
        &format_agent_terminal_output(status, result, error.as_deref()),
    )?;

    let mut next_manifest = manifest.clone();
    next_manifest.status = status.to_string();
    next_manifest.completed_at = Some(iso8601_now());
    next_manifest.error = error;

    write_agent_manifest(&next_manifest)
}

/**
 * append_agent_output - 追加内容到Agent输出文件
 */
fn append_agent_output(path: &str, suffix: &str) -> Result<(), String> {
    use std::io::Write as _;

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;

    file.write_all(suffix.as_bytes())
        .map_err(|error| error.to_string())
}

/**
 * format_agent_terminal_output - 格式化Agent终端输出
 */
fn format_agent_terminal_output(status: &str, result: Option<&str>, error: Option<&str>) -> String {
    let mut sections = vec![format!("\n## Result\n\n- status: {status}\n")];

    if let Some(result) = result.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("\n### Final response\n\n{}\n", result.trim()));
    }

    if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("\n### Error\n\n{}\n", error.trim()));
    }

    sections.join("")
}

/**
 * final_assistant_text - 从TurnSummary提取最终的Assistant文本
 */
fn final_assistant_text(summary: &TurnSummary) -> String {
    summary
        .assistant_messages
        .last()
        .and_then(|msg| {
            msg.blocks.iter().find_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
        })
        .unwrap_or_default()
}

/**
 * make_agent_id - 生成唯一的Agent ID
 */
fn make_agent_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();

    format!("agent-{:016x}", nanos)
}

/**
 * iso8601_now - 返回当前时间的ISO8601格式字符串
 */
fn iso8601_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch");

    // 简化格式：Unix timestamp with nanoseconds
    format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos())
}

/**
 * agent_store_dir - 获取Agent存储目录
 */
fn agent_store_dir() -> Result<PathBuf, String> {
    let base = std::env::var("AGENT_STORE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(".claw_agents")
        });

    Ok(base)
}

/**
 * normalize_subagent_type - 标准化子Agent类型名称
 */
fn normalize_subagent_type(subagent_type: Option<&str>) -> String {
    subagent_type
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("default")
        .to_lowercase()
}

/**
 * slugify_agent_name - 将字符串转换为适合文件名的格式
 */
fn slugify_agent_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            ' ' | '\t' | '\n' => '-',
            _ => '_',
        })
        .collect()
}

/**
 * resolve_agent_model - 解析Agent使用的模型
 */
fn resolve_agent_model(model: Option<&str>) -> String {
    model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or(DEFAULT_AGENT_MODEL)
        .to_string()
}

// ============================================================
// 测试模块
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_tools_explore_vs_default() {
        // Explore 类型应该没有 bash 权限
        let explore_tools = allowed_tools_for_subagent("Explore");
        assert!(!explore_tools.contains("bash"));
        assert!(explore_tools.contains("read_file"));

        // 默认类型应该有 bash 权限
        let default_tools = allowed_tools_for_subagent("default");
        assert!(default_tools.contains("bash"));
    }

    #[test]
    fn test_subagent_executor_blocks_disallowed() {
        // 只允许 read_file
        let allowed = btreeset!["read_file".to_string()];
        let mut executor = SubagentToolExecutor::new(allowed);

        // 尝试执行被禁止的工具
        let result = executor.execute("bash", r#"{"command":"ls"}"#);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("is not enabled for this sub-agent"));
    }

    #[test]
    fn test_normalize_subagent_type() {
        assert_eq!(normalize_subagent_type(Some("Explore")), "explore");
        assert_eq!(normalize_subagent_type(Some(" EXPLORE ")), "explore");
        assert_eq!(normalize_subagent_type(None), "default");
        assert_eq!(normalize_subagent_type(Some("")), "default");
    }

    #[test]
    fn test_slugify_agent_name() {
        assert_eq!(slugify_agent_name("My Agent"), "My-Agent");
        assert_eq!(slugify_agent_name("test@123"), "test_123");
        assert_eq!(slugify_agent_name("file.py"), "file.py");
    }

    #[test]
    fn test_mvp_tools_count() {
        let tools = mvp_tool_specs();
        assert!(tools.len() >= 15, "MVP should have at least 15 tools");

        // 验证关键工具存在
        let tool_names: Vec<_> = tools.iter().map(|t| t.name).collect();
        assert!(tool_names.contains(&"bash"));
        assert!(tool_names.contains(&"read_file"));
        assert!(tool_names.contains(&"Agent"));
    }
}

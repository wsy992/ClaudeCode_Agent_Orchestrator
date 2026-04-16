/**
 * @file permissions.rs
 * @brief ClaudeCode Agent Orchestrator - 权限系统实现
 *
 * 本文件实现了细粒度的权限控制机制：
 * - PermissionMode: 权限级别枚举
 * - PermissionRequest: 权限请求
 * - PermissionPolicy: 权限策略
 * - PermissionPrompter: 权限提示接口
 *
 * 权限系统确保工具执行符合安全策略，防止未授权操作
 *
 * @author ClaudeCode Research Team
 * @date 2026-04-14
 */

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

// ============================================================
// 权限级别枚举
// ============================================================

/**
 * PermissionMode - 权限级别枚举
 *
 * # 级别说明（从低到高）
 * - ReadOnly: 只读操作，不允许修改
 * - WorkspaceWrite: 工作区写操作
 * - DangerFullAccess: 危险的全权限操作
 * - Prompt: 需要用户确认
 * - Allow: 允许所有操作
 *
 * # 设计理念
 * 1. 渐进式权限：操作需要满足最低权限要求
 * 2. 安全默认值：DangerFullAccess 作为大多数工具的默认要求
 * 3. 用户控制：Prompt 级别允许用户动态授权
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionMode {
    /// 只读 - 文件读取、搜索等
    ReadOnly,

    /// 工作区写 - 文件编辑、创建等
    WorkspaceWrite,

    /// 危险的全权限 - bash 执行等
    DangerFullAccess,

    /// 需要提示 - 需用户确认
    Prompt,

    /// 允许所有 - 完全信任
    Allow,
}

impl PermissionMode {
    /**
     * 获取权限级别的字符串表示
     */
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
            Self::Prompt => "prompt",
            Self::Allow => "allow",
        }
    }
}

// ============================================================
// 权限请求
// ============================================================

/**
 * PermissionRequest - 权限请求结构
 *
 * # 字段说明
 * - tool_name: 请求执行的工具名称
 * - input: 工具输入参数
 * - current_mode: 当前权限模式
 * - required_mode: 执行所需的最低权限模式
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub input: String,
    pub current_mode: PermissionMode,
    pub required_mode: PermissionMode,
}

/**
 * PermissionPromptDecision - 权限提示决策
 *
 * # 变体说明
 * - Allow: 用户允许执行
 * - Deny: 用户拒绝执行（带原因）
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionPromptDecision {
    Allow,
    Deny { reason: String },
}

/**
 * PermissionPrompter trait - 权限提示接口
 *
 * # 设计目的
 * 抽象权限提示逻辑，允许不同的 UI 实现
 *
 * # 使用场景
 * - CLI: 交互式终端提示
 * - GUI: 图形对话框
 * - API: 返回固定决策
 */
pub trait PermissionPrompter {
    /**
     * 向用户发出权限提示并获取决策
     *
     * @param request 权限请求详情
     * @return PermissionPromptDecision 用户决策
     */
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision;
}

// ============================================================
// 权限结果
// ============================================================

/**
 * PermissionOutcome - 权限检查结果
 *
 * # 变体说明
 * - Allow: 允许执行
 * - Deny: 拒绝执行（带原因）
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOutcome {
    Allow,
    Deny { reason: String },
}

// ============================================================
// 权限策略
// ============================================================

/**
 * PermissionPolicy - 权限策略
 *
 * # 功能
 * 1. 管理当前权限模式
 * 2. 定义特定工具的权限要求
 * 3. 授权决策逻辑
 *
 * # 使用示例
 * ```rust
 * let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
 *     .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
 *     .with_tool_requirement("read_file", PermissionMode::ReadOnly);
 *
 * let outcome = policy.authorize("bash", "{}", None);
 * // -> Deny { reason: "tool 'bash' requires danger-full-access permission" }
 * ```
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicy {
    /// 当前权限模式
    active_mode: PermissionMode,

    /// 特定工具的权限要求
    tool_requirements: BTreeMap<String, PermissionMode>,
}

impl PermissionPolicy {
    /**
     * 创建新的权限策略
     *
     * @param active_mode 初始权限模式
     */
    #[must_use]
    pub fn new(active_mode: PermissionMode) -> Self {
        Self {
            active_mode,
            tool_requirements: BTreeMap::new(),
        }
    }

    /**
     * 设置特定工具的权限要求
     *
     * @param tool_name 工具名称
     * @param required_mode 所需权限模式
     * @return Self 返回自身以支持链式调用
     */
    #[must_use]
    pub fn with_tool_requirement(
        mut self,
        tool_name: impl Into<String>,
        required_mode: PermissionMode,
    ) -> Self {
        self.tool_requirements
            .insert(tool_name.into(), required_mode);
        self
    }

    /**
     * 获取当前权限模式
     */
    #[must_use]
    pub fn active_mode(&self) -> PermissionMode {
        self.active_mode
    }

    /**
     * 获取工具所需的权限模式
     *
     * 如果没有特定要求，返回 DangerFullAccess
     */
    #[must_use]
    pub fn required_mode_for(&self, tool_name: &str) -> PermissionMode {
        self.tool_requirements
            .get(tool_name)
            .copied()
            .unwrap_or(PermissionMode::DangerFullAccess)
    }

    /**
     * 授权决策
     *
     * # 算法
     * 1. 如果当前模式为 Allow，或当前模式 >= 所需模式，允许
     * 2. 如果当前模式为 Prompt 或需要升级到 DangerFullAccess，提示用户
     * 3. 否则拒绝
     *
     * @param tool_name 工具名称
     * @param input 工具输入
     * @param prompter 权限提示器（可选）
     */
    #[must_use]
    pub fn authorize(
        &self,
        tool_name: &str,
        input: &str,
        mut prompter: Option<&mut dyn PermissionPrompter>,
    ) -> PermissionOutcome {
        let current_mode = self.active_mode();
        let required_mode = self.required_mode_for(tool_name);

        // Step 1: 权限足够，直接允许
        if current_mode == PermissionMode::Allow || current_mode >= required_mode {
            return PermissionOutcome::Allow;
        }

        // Step 2: 构建权限请求
        let request = PermissionRequest {
            tool_name: tool_name.to_string(),
            input: input.to_string(),
            current_mode,
            required_mode,
        };

        // Step 3: 需要提示用户
        if current_mode == PermissionMode::Prompt
            || (current_mode == PermissionMode::WorkspaceWrite
                && required_mode == PermissionMode::DangerFullAccess)
        {
            return match prompter.as_mut() {
                Some(prompter) => match prompter.decide(&request) {
                    PermissionPromptDecision::Allow => PermissionOutcome::Allow,
                    PermissionPromptDecision::Deny { reason } => {
                        PermissionOutcome::Deny { reason }
                    }
                },
                None => PermissionOutcome::Deny {
                    reason: format!(
                        "tool '{}' requires approval to escalate from {} to {}",
                        tool_name,
                        current_mode.as_str(),
                        required_mode.as_str()
                    ),
                },
            };
        }

        // Step 4: 权限不足，拒绝
        PermissionOutcome::Deny {
            reason: format!(
                "tool '{}' requires {} permission; current mode is {}",
                tool_name,
                required_mode.as_str(),
                current_mode.as_str()
            ),
        }
    }
}

// ============================================================
// RecordingPrompter - 记录型提示器（用于测试）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingPrompter {
        seen: Vec<PermissionRequest>,
        decision: PermissionPromptDecision,
    }

    impl RecordingPrompter {
        fn allowing() -> Self {
            Self {
                seen: Vec::new(),
                decision: PermissionPromptDecision::Allow,
            }
        }

        fn denying(reason: &str) -> Self {
            Self {
                seen: Vec::new(),
                decision: PermissionPromptDecision::Deny {
                    reason: reason.to_string(),
                },
            }
        }
    }

    impl PermissionPrompter for RecordingPrompter {
        fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
            self.seen.push(request.clone());
            self.decision.clone()
        }
    }

    // ============================================================
    // 权限策略测试
    // ============================================================

    #[test]
    fn test_allow_when_mode_is_allow() {
        // 给定：权限模式为 Allow
        let policy = PermissionPolicy::new(PermissionMode::Allow);

        // 当：授权任何工具
        let outcome = policy.authorize("bash", "{}", None);

        // 那么：应该允许
        assert_eq!(outcome, PermissionOutcome::Allow);
    }

    #[test]
    fn test_allow_when_current_mode_meets_requirement() {
        // 给定：WorkspaceWrite 模式，bash 需要 DangerFullAccess
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        // 当：授权 read_file（只读）
        let outcome = policy.authorize("read_file", "{}", None);

        // 那么：应该允许（WorkspaceWrite >= ReadOnly）
        assert_eq!(outcome, PermissionOutcome::Allow);
    }

    #[test]
    fn test_deny_when_insufficient_permission() {
        // 给定：ReadOnly 模式
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite);

        // 当：授权 write_file
        let outcome = policy.authorize("write_file", "{}", None);

        // 那么：应该拒绝
        assert!(matches!(
            outcome,
            PermissionOutcome::Deny { reason }
            if reason.contains("requires workspace-write permission")
        ));
    }

    #[test]
    fn test_prompt_for_escalation() {
        // 给定：WorkspaceWrite 模式，需要 DangerFullAccess
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        let mut prompter = RecordingPrompter::allowing();

        // 当：授权 bash
        let outcome = policy.authorize("bash", "echo hi", Some(&mut prompter));

        // 那么：应该允许（用户批准）
        assert_eq!(outcome, PermissionOutcome::Allow);

        // 验证提示器收到了请求
        assert_eq!(prompter.seen.len(), 1);
        assert_eq!(prompter.seen[0].tool_name, "bash");
        assert_eq!(prompter.seen[0].current_mode, PermissionMode::WorkspaceWrite);
        assert_eq!(
            prompter.seen[0].required_mode,
            PermissionMode::DangerFullAccess
        );
    }

    #[test]
    fn test_prompt_denial() {
        // 给定：WorkspaceWrite 模式，需要 DangerFullAccess
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        let mut prompter = RecordingPrompter::denying("not now");

        // 当：授权 bash
        let outcome = policy.authorize("bash", "echo hi", Some(&mut prompter));

        // 那么：应该拒绝
        assert!(matches!(
            outcome,
            PermissionOutcome::Deny { reason }
            if reason == "not now"
        ));
    }

    #[test]
    fn test_no_prompter_returns_deny() {
        // 给定：WorkspaceWrite 模式，需要 DangerFullAccess，但没有 prompter
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        // 当：授权 bash（无 prompter）
        let outcome = policy.authorize("bash", "echo hi", None);

        // 那么：应该拒绝
        assert!(matches!(
            outcome,
            PermissionOutcome::Deny { reason }
            if reason.contains("requires approval to escalate")
        ));
    }

    #[test]
    fn test_default_requirement_is_danger_full_access() {
        // 给定：默认策略
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);

        // 当：授权未知工具
        let outcome = policy.authorize("unknown_tool", "{}", None);

        // 那么：应该拒绝（默认需要 DangerFullAccess）
        assert!(matches!(
            outcome,
            PermissionOutcome::Deny { reason }
            if reason.contains("requires danger-full-access permission")
        ));
    }

    #[test]
    fn test_permission_mode_ordering() {
        // 验证权限级别顺序
        assert!(PermissionMode::Allow > PermissionMode::Prompt);
        assert!(PermissionMode::Prompt > PermissionMode::DangerFullAccess);
        assert!(PermissionMode::DangerFullAccess > PermissionMode::WorkspaceWrite);
        assert!(PermissionMode::WorkspaceWrite > PermissionMode::ReadOnly);
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
    fn test_tool_requirement_chain() {
        // 验证链式调用
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        assert_eq!(
            policy.required_mode_for("read_file"),
            PermissionMode::ReadOnly
        );
        assert_eq!(
            policy.required_mode_for("write_file"),
            PermissionMode::WorkspaceWrite
        );
        assert_eq!(
            policy.required_mode_for("bash"),
            PermissionMode::DangerFullAccess
        );
        assert_eq!(
            policy.required_mode_for("unknown"),
            PermissionMode::DangerFullAccess // 默认值
        );
    }
}

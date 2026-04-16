/**
 * @file session.rs
 * @brief ClaudeCode Agent Orchestrator - 会话与消息结构
 *
 * 本文件定义了 Agent 对话系统的核心数据结构：
 * - Session: 对话会话
 * - ConversationMessage: 对话消息
 * - ContentBlock: 消息内容块
 * - MessageRole: 消息角色
 *
 * @author ClaudeCode Research Team
 * @date 2026-04-14
 */

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;

// ============================================================
// 消息角色枚举
// ============================================================

/**
 * MessageRole - 消息角色枚举
 *
 * # 变体说明
 * - System: 系统消息（系统提示词等）
 * - User: 用户消息
 * - Assistant: 助手消息
 * - Tool: 工具结果消息
 */
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

// ============================================================
// 内容块枚举
// ============================================================

/**
 * ContentBlock - 消息内容块枚举
 *
 * # 变体说明
 * - Text: 纯文本内容
 * - ToolUse: 工具调用请求
 * - ToolResult: 工具执行结果
 */
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },

    ToolUse {
        id: String,
        name: String,
        input: String,
    },

    ToolResult {
        tool_use_id: String,
        tool_name: String,
        output: String,
        is_error: bool,
    },
}

// ============================================================
// 对话消息
// ============================================================

/**
 * ConversationMessage - 对话消息结构
 *
 * # 字段说明
 * - role: 消息角色
 * - blocks: 内容块列表
 * - usage: Token 使用量（可选）
 */
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub blocks: Vec<ContentBlock>,
    pub usage: Option<TokenUsage>,
}

// ============================================================
// 会话结构
// ============================================================

/**
 * Session - 对话会话结构
 *
 * # 功能
 * 1. 存储对话历史
 * 2. 版本化以支持序列化
 * 3. 持久化和恢复
 *
 * # 版本控制
 * version 字段用于处理序列化格式的演进
 */
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub version: u32,
    pub messages: Vec<ConversationMessage>,
}

// ============================================================
// 错误类型
// ============================================================

/**
 * SessionError - 会话相关错误
 */
#[derive(Debug)]
pub enum SessionError {
    Io(std::io::Error),
    Json(JsonError),
    Format(String),
}

impl Display for SessionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::Format(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<std::io::Error> for SessionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<JsonError> for SessionError {
    fn from(value: JsonError) -> Self {
        Self::Json(value)
    }
}

// ============================================================
// Session 实现
// ============================================================

impl Session {
    /**
     * 创建新的空会话
     */
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: 1,
            messages: Vec::new(),
        }
    }

    /**
     * 保存会话到文件
     */
    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        fs::write(path, self.to_json().render())?;
        Ok(())
    }

    /**
     * 从文件加载会话
     */
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let contents = fs::read_to_string(path)?;
        Self::from_json(&JsonValue::parse(&contents)?)
    }

    /**
     * 序列化为 JSON
     */
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let mut object = BTreeMap::new();
        object.insert(
            "version".to_string(),
            JsonValue::Number(i64::from(self.version)),
        );
        object.insert(
            "messages".to_string(),
            JsonValue::Array(
                self.messages
                    .iter()
                    .map(ConversationMessage::to_json)
                    .collect(),
            ),
        );
        JsonValue::Object(object)
    }

    /**
     * 从 JSON 反序列化
     */
    pub fn from_json(value: &JsonValue) -> Result<Self, SessionError> {
        let object = value.as_object()
            .ok_or_else(|| SessionError::Format("session must be an object".to_string()))?;

        let version = object.get("version")
            .and_then(JsonValue::as_i64)
            .ok_or_else(|| SessionError::Format("missing version".to_string()))?;

        let version = u32::try_from(version)
            .map_err(|_| SessionError::Format("version out of range".to_string()))?;

        let messages = object.get("messages")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| SessionError::Format("missing messages".to_string()))?
            .iter()
            .map(ConversationMessage::from_json)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { version, messages })
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// ConversationMessage 实现
// ============================================================

impl ConversationMessage {
    /**
     * 创建用户文本消息
     */
    #[must_use]
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            blocks: vec![ContentBlock::Text { text: text.into() }],
            usage: None,
        }
    }

    /**
     * 创建助手消息
     */
    #[must_use]
    pub fn assistant(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: MessageRole::Assistant,
            blocks,
            usage: None,
        }
    }

    /**
     * 创建带使用量的助手消息
     */
    #[must_use]
    pub fn assistant_with_usage(blocks: Vec<ContentBlock>, usage: Option<TokenUsage>) -> Self {
        Self {
            role: MessageRole::Assistant,
            blocks,
            usage,
        }
    }

    /**
     * 创建工具结果消息
     */
    #[must_use]
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        tool_name: impl Into<String>,
        output: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            role: MessageRole::Tool,
            blocks: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                tool_name: tool_name.into(),
                output: output.into(),
                is_error,
            }],
            usage: None,
        }
    }

    /**
     * 序列化为 JSON
     */
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let mut object = BTreeMap::new();
        object.insert(
            "role".to_string(),
            JsonValue::String(match self.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            }.to_string()),
        );
        object.insert(
            "blocks".to_string(),
            JsonValue::Array(self.blocks.iter().map(ContentBlock::to_json).collect()),
        );
        if let Some(usage) = self.usage {
            object.insert("usage".to_string(), usage_to_json(usage));
        }
        JsonValue::Object(object)
    }

    /**
     * 从 JSON 反序列化
     */
    fn from_json(value: &JsonValue) -> Result<Self, SessionError> {
        let object = value.as_object()
            .ok_or_else(|| SessionError::Format("message must be an object".to_string()))?;

        let role = match object.get("role")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| SessionError::Format("missing role".to_string()))?
        {
            "system" => MessageRole::System,
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "tool" => MessageRole::Tool,
            other => return Err(SessionError::Format(format!("unsupported message role: {other}"))),
        };

        let blocks = object.get("blocks")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| SessionError::Format("missing blocks".to_string()))?
            .iter()
            .map(ContentBlock::from_json)
            .collect::<Result<Vec<_>, _>>()?;

        let usage = object.get("usage").map(usage_from_json).transpose()?;

        Ok(Self { role, blocks, usage })
    }
}

// ============================================================
// ContentBlock 实现
// ============================================================

impl ContentBlock {
    /**
     * 序列化为 JSON
     */
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let mut object = BTreeMap::new();
        match self {
            Self::Text { text } => {
                object.insert("type".to_string(), JsonValue::String("text".to_string()));
                object.insert("text".to_string(), JsonValue::String(text.clone()));
            }
            Self::ToolUse { id, name, input } => {
                object.insert("type".to_string(), JsonValue::String("tool_use".to_string()));
                object.insert("id".to_string(), JsonValue::String(id.clone()));
                object.insert("name".to_string(), JsonValue::String(name.clone()));
                object.insert("input".to_string(), JsonValue::String(input.clone()));
            }
            Self::ToolResult { tool_use_id, tool_name, output, is_error } => {
                object.insert("type".to_string(), JsonValue::String("tool_result".to_string()));
                object.insert("tool_use_id".to_string(), JsonValue::String(tool_use_id.clone()));
                object.insert("tool_name".to_string(), JsonValue::String(tool_name.clone()));
                object.insert("output".to_string(), JsonValue::String(output.clone()));
                object.insert("is_error".to_string(), JsonValue::Bool(*is_error));
            }
        }
        JsonValue::Object(object)
    }

    /**
     * 从 JSON 反序列化
     */
    fn from_json(value: &JsonValue) -> Result<Self, SessionError> {
        let object = value.as_object()
            .ok_or_else(|| SessionError::Format("block must be an object".to_string()))?;

        match object.get("type")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| SessionError::Format("missing block type".to_string()))?
        {
            "text" => Ok(Self::Text { text: required_string(object, "text")? }),
            "tool_use" => Ok(Self::ToolUse {
                id: required_string(object, "id")?,
                name: required_string(object, "name")?,
                input: required_string(object, "input")?,
            }),
            "tool_result" => Ok(Self::ToolResult {
                tool_use_id: required_string(object, "tool_use_id")?,
                tool_name: required_string(object, "tool_name")?,
                output: required_string(object, "output")?,
                is_error: object.get("is_error")
                    .and_then(JsonValue::as_bool)
                    .ok_or_else(|| SessionError::Format("missing is_error".to_string()))?,
            }),
            other => Err(SessionError::Format(format!("unsupported block type: {other}"))),
        }
    }
}

// ============================================================
// 辅助函数
// ============================================================

fn usage_to_json(usage: TokenUsage) -> JsonValue {
    let mut object = BTreeMap::new();
    object.insert("input_tokens".to_string(), JsonValue::Number(i64::from(usage.input_tokens)));
    object.insert("output_tokens".to_string(), JsonValue::Number(i64::from(usage.output_tokens)));
    object.insert("cache_creation_input_tokens".to_string(), JsonValue::Number(i64::from(usage.cache_creation_input_tokens)));
    object.insert("cache_read_input_tokens".to_string(), JsonValue::Number(i64::from(usage.cache_read_input_tokens)));
    JsonValue::Object(object)
}

fn usage_from_json(value: &JsonValue) -> Result<TokenUsage, SessionError> {
    let object = value.as_object()
        .ok_or_else(|| SessionError::Format("usage must be an object".to_string()))?;
    Ok(TokenUsage {
        input_tokens: required_u32(object, "input_tokens")?,
        output_tokens: required_u32(object, "output_tokens")?,
        cache_creation_input_tokens: required_u32(object, "cache_creation_input_tokens")?,
        cache_read_input_tokens: required_u32(object, "cache_read_input_tokens")?,
    })
}

fn required_string(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<String, SessionError> {
    object.get(key)
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| SessionError::Format(format!("missing {key}")))
}

fn required_u32(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<u32, SessionError> {
    let value = object.get(key)
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| SessionError::Format(format!("missing {key}")))?;
    u32::try_from(value).map_err(|_| SessionError::Format(format!("{key} out of range")))
}

// ============================================================
// TokenUsage 结构
// ============================================================

/**
 * TokenUsage - Token 使用量统计
 */
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32,
}

impl TokenUsage {
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

// ============================================================
// JSON 类型（简化版）
// ============================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl JsonValue {
    pub fn parse(s: &str) -> Result<Self, JsonError> {
        // 简化实现
        serde_json::from_str(s).map_err(JsonError::from)
    }

    pub fn render(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Self>> {
        match self {
            Self::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Self>> {
        match self {
            Self::Array(arr) => Some(arr),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum JsonError {
    Parse(serde_json::Error),
}

impl From<serde_json::Error> for JsonError {
    fn from(value: serde_json::Error) -> Self {
        Self::Parse(value)
    }
}

// Placeholder serde imports
mod serde {
    pub use ::serde::*;

    pub mod json {
        pub use ::serde_json::*;
    }

    pub mod Serialize {
        pub use ::serde::Serialize;
    }

    pub mod Deserialize {
        pub use ::serde::Deserialize;
    }
}

// ============================================================
// 测试模块
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_new() {
        let session = Session::new();
        assert_eq!(session.version, 1);
        assert!(session.messages.is_empty());
    }

    #[test]
    fn test_user_text_message() {
        let msg = ConversationMessage::user_text("Hello");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.blocks.len(), 1);
        match &msg.blocks[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn test_tool_result_message() {
        let msg = ConversationMessage::tool_result("tool-1", "bash", "hello", false);
        assert_eq!(msg.role, MessageRole::Tool);
        match &msg.blocks[0] {
            ContentBlock::ToolResult { tool_use_id, tool_name, output, is_error } => {
                assert_eq!(tool_use_id, "tool-1");
                assert_eq!(tool_name, "bash");
                assert_eq!(output, "hello");
                assert!(!*is_error);
            }
            _ => panic!("expected ToolResult block"),
        }
    }

    #[test]
    fn test_session_persistence() {
        let mut session = Session::new();
        session.messages.push(ConversationMessage::user_text("hello"));
        session.messages.push(ConversationMessage::assistant(vec![
            ContentBlock::Text { text: "thinking".to_string() },
            ContentBlock::ToolUse { id: "tool-1".to_string(), name: "bash".to_string(), input: "echo hi".to_string() },
        ]));
        session.messages.push(ConversationMessage::tool_result("tool-1", "bash", "hi", false));

        // 保存到临时文件
        let path = std::env::temp_dir().join("test-session.json");
        session.save_to_path(&path).expect("session should save");

        // 从文件加载
        let restored = Session::load_from_path(&path).expect("session should load");

        // 验证
        assert_eq!(restored, session);

        // 清理
        fs::remove_file(&path).expect("temp file should be removable");
    }

    #[test]
    fn test_content_block_json_roundtrip() {
        let blocks = vec![
            ContentBlock::Text { text: "Hello".to_string() },
            ContentBlock::ToolUse { id: "t1".to_string(), name: "read".to_string(), input: "{}".to_string() },
            ContentBlock::ToolResult { tool_use_id: "t1".to_string(), tool_name: "read".to_string(), output: "result".to_string(), is_error: false },
        ];

        for block in blocks {
            let json = block.to_json();
            let restored = ContentBlock::from_json(&json).expect("should deserialize");
            assert_eq!(restored, block);
        }
    }
}

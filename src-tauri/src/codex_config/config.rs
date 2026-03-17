//! Codex 配置文件解析
//!
//! 支持读写 config.toml 配置文件，兼容标准 Codex CLI 格式。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Codex 主配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CodexConfig {
    /// 模型提供商 (openai, anthropic, etc.)
    pub model_provider: String,
    /// 模型名称 (gpt-5.4, claude-3, etc.)
    pub model: String,
    /// 推理强度 (xhigh, high, medium, low)
    pub model_reasoning_effort: String,
    /// 人格风格 (pragmatic, neutral, friendly, etc.)
    pub personality: String,
    /// 是否禁用响应存储
    #[serde(default)]
    pub disable_response_storage: bool,
    /// 首选认证方式 (apikey, oauth)
    #[serde(default = "default_auth_method")]
    pub preferred_auth_method: String,
    /// MCP 服务器配置
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
    /// 项目配置
    #[serde(default)]
    pub projects: HashMap<String, ProjectConfig>,
}

fn default_auth_method() -> String {
    "apikey".to_string()
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            model_provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            model_reasoning_effort: "medium".to_string(),
            personality: "pragmatic".to_string(),
            disable_response_storage: false,
            preferred_auth_method: "apikey".to_string(),
            mcp_servers: HashMap::new(),
            projects: HashMap::new(),
        }
    }
}

/// MCP 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// 服务器类型 (stdio, sse, etc.)
    #[serde(rename = "type")]
    pub server_type: String,
    /// 命令
    pub command: String,
    /// 命令参数
    #[serde(default)]
    pub args: Vec<String>,
    /// 环境变量
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// 项目配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// 信任级别 (trusted, untrusted)
    pub trust_level: String,
}

impl CodexConfig {
    /// 从文件加载配置
    pub fn load(config_path: &Path) -> Result<Self, String> {
        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(config_path)
            .map_err(|e| format!("读取配置文件失败: {}", e))?;

        toml::from_str(&content).map_err(|e| format!("解析配置文件失败: {}", e))
    }

    /// 从 Codex home 目录加载配置
    pub fn load_from_home(codex_home: &Path) -> Result<Self, String> {
        let config_path = codex_home.join("config.toml");
        Self::load(&config_path)
    }

    /// 保存配置到文件
    pub fn save(&self, config_path: &Path) -> Result<(), String> {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建配置目录失败: {}", e))?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("序列化配置失败: {}", e))?;

        fs::write(config_path, content).map_err(|e| format!("写入配置文件失败: {}", e))
    }

    /// 保存到 Codex home 目录
    pub fn save_to_home(&self, codex_home: &Path) -> Result<(), String> {
        let config_path = codex_home.join("config.toml");
        self.save(&config_path)
    }

    /// 获取 Codex CLI 命令行参数
    pub fn to_cli_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        // 模型参数
        if !self.model.is_empty() {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }

        // 推理强度
        if !self.model_reasoning_effort.is_empty() && self.model_reasoning_effort != "medium" {
            args.push("--reasoning-effort".to_string());
            args.push(self.model_reasoning_effort.clone());
        }

        args
    }

    /// 获取人格提示词
    pub fn personality_prompt(&self) -> Option<String> {
        match self.personality.as_str() {
            "pragmatic" => Some(
                "你是一个务实的助手，专注于解决问题，回答简洁明了。".to_string(),
            ),
            "friendly" => Some(
                "你是一个友好热情的助手，用亲切的语气与用户交流。".to_string(),
            ),
            "professional" => Some(
                "你是一个专业的助手，使用正式的语言和结构化的回答。".to_string(),
            ),
            "neutral" | "" => None,
            custom => Some(format!("你的人格风格是: {}", custom)),
        }
    }

    /// 检查项目是否受信任
    pub fn is_project_trusted(&self, project_path: &str) -> bool {
        // 检查精确匹配
        if let Some(config) = self.projects.get(project_path) {
            return config.trust_level == "trusted";
        }

        // 检查父目录匹配
        let path = PathBuf::from(project_path);
        for (pattern, config) in &self.projects {
            let pattern_path = PathBuf::from(pattern);
            if path.starts_with(&pattern_path) && config.trust_level == "trusted" {
                return true;
            }
        }

        false
    }
}

/// 创建默认配置文件（如果不存在）
pub fn ensure_default_config(codex_home: &Path) -> Result<CodexConfig, String> {
    let config_path = codex_home.join("config.toml");
    if config_path.exists() {
        CodexConfig::load(&config_path)
    } else {
        let config = CodexConfig::default();
        config.save(&config_path)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CodexConfig::default();
        assert_eq!(config.model_provider, "openai");
        assert_eq!(config.model_reasoning_effort, "medium");
    }

    #[test]
    fn test_personality_prompt() {
        let mut config = CodexConfig::default();
        config.personality = "pragmatic".to_string();
        assert!(config.personality_prompt().is_some());

        config.personality = "neutral".to_string();
        assert!(config.personality_prompt().is_none());
    }

    #[test]
    fn test_parse_config() {
        let toml_str = r#"
model_provider = "openai"
model = "gpt-5.4"
model_reasoning_effort = "high"
personality = "pragmatic"

[mcp_servers.test]
type = "stdio"
command = "npx"
args = ["-y", "test-server"]

[projects."/home/user/project"]
trust_level = "trusted"
"#;
        let config: CodexConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.model, "gpt-5.4");
        assert_eq!(config.model_reasoning_effort, "high");
        assert!(config.mcp_servers.contains_key("test"));
        assert!(config.is_project_trusted("/home/user/project"));
    }
}

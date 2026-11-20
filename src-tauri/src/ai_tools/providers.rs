use std::{
    env,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item, Table};

use super::storage::{read_config, Change};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Client {
    Codex,
    Claude,
    Gemini,
    Dsh,
    Workbuddy,
    #[serde(rename = "claude-desktop")]
    ClaudeDesktop,
    Grokbuild,
    Opencode,
    Openclaw,
    Hermes,
    Pi,
    Mcode,
    Kimi,
    Agy,
    Omp,
    Copilot,
}

impl Client {
    pub const ALL: [Self; 16] = [
        Self::Codex,
        Self::Claude,
        Self::ClaudeDesktop,
        Self::Gemini,
        Self::Dsh,
        Self::Workbuddy,
        Self::Copilot,
        Self::Grokbuild,
        Self::Opencode,
        Self::Openclaw,
        Self::Hermes,
        Self::Pi,
        Self::Mcode,
        Self::Kimi,
        Self::Agy,
        Self::Omp,
    ];

    pub fn has_session_logs(self) -> bool {
        self.has_usage_logs()
            || matches!(
                self,
                Self::Agy | Self::Opencode | Self::Hermes | Self::Mcode
            )
    }

    pub fn has_usage_logs(self) -> bool {
        matches!(
            self,
            Self::Codex
                | Self::Claude
                | Self::Gemini
                | Self::Dsh
                | Self::Workbuddy
                | Self::Pi
                | Self::Omp
                | Self::Openclaw
                | Self::Copilot
                | Self::Kimi
                | Self::Grokbuild
        )
    }

    pub fn manages_config(self) -> bool {
        matches!(self, Self::Codex | Self::Claude | Self::Gemini)
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Dsh => "dsh",
            Self::Workbuddy => "workbuddy",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Grokbuild => "grokbuild",
            Self::Opencode => "opencode",
            Self::Openclaw => "openclaw",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::Mcode => "mcode",
            Self::Kimi => "kimi",
            Self::Agy => "agy",
            Self::Omp => "omp",
            Self::Copilot => "copilot",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::Gemini => "Gemini CLI",
            Self::Dsh => "DeepSeek Harness",
            Self::Workbuddy => "WorkBuddy",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Grokbuild => "Grok Build",
            Self::Opencode => "OpenCode",
            Self::Openclaw => "OpenClaw",
            Self::Hermes => "Hermes Agent",
            Self::Pi => "Pi",
            Self::Mcode => "MiniMax Code",
            Self::Kimi => "Kimi Code",
            Self::Agy => "Antigravity CLI",
            Self::Omp => "Oh My Pi",
            Self::Copilot => "GitHub Copilot CLI",
        }
    }

    pub fn home(self, user: &Path) -> PathBuf {
        let var = match self {
            Self::Codex => "CODEX_HOME",
            Self::Claude => "CLAUDE_CONFIG_DIR",
            Self::Gemini => "GEMINI_CLI_HOME",
            Self::Dsh => "DSH_HOME",
            Self::Workbuddy => "WORKBUDDY_CONFIG_DIR",
            Self::ClaudeDesktop => "",
            Self::Grokbuild => "GROK_HOME",
            Self::Opencode => "OPENCODE_CONFIG_DIR",
            Self::Openclaw => "OPENCLAW_STATE_DIR",
            Self::Hermes => "HERMES_HOME",
            Self::Pi => "PI_CODING_AGENT_DIR",
            Self::Mcode => "MCODE_HOME",
            Self::Kimi => "KIMI_CODE_HOME",
            Self::Agy => "",
            Self::Omp => "OMP_AGENT_DIR",
            Self::Copilot => "COPILOT_HOME",
        };
        env::var_os(var)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| {
                if self == Self::Gemini {
                    p.join(".gemini")
                } else {
                    p
                }
            })
            .unwrap_or_else(|| match self {
                Self::ClaudeDesktop => user.join("AppData/Local/Claude"),
                Self::Grokbuild => user.join(".grok"),
                Self::Opencode => user.join(".config/opencode"),
                Self::Openclaw => user.join(".openclaw"),
                Self::Hermes => user.join(".hermes"),
                Self::Pi => user.join(".pi/agent"),
                Self::Mcode => user.join(".minimax"),
                Self::Kimi => user.join(".kimi-code"),
                Self::Agy => user.join(".gemini/antigravity-cli"),
                Self::Omp => user.join(".omp/agent"),
                Self::Copilot => user.join(".copilot"),
                _ => user.join(format!(".{}", self.id())),
            })
    }

    pub fn config(self, user: &Path) -> PathBuf {
        let home = self.home(user);
        home.join(match self {
            Self::Codex => "config.toml",
            Self::Dsh if !home.join("cordis.patch.yml").is_file() => {
                "profiles/desktop/cordis.patch.yml"
            }
            Self::Dsh => "cordis.patch.yml",
            Self::Workbuddy => "models.json",
            Self::ClaudeDesktop => "claude_desktop_config.json",
            Self::Grokbuild => "config.toml",
            Self::Opencode => "opencode.json",
            Self::Openclaw => "openclaw.json",
            Self::Hermes => "config.yaml",
            Self::Pi => "models.json",
            Self::Mcode => "config.yaml",
            Self::Kimi => "config.toml",
            Self::Agy => "settings.json",
            Self::Omp => "models.yml",
            Self::Copilot => "settings.json",
            _ => "settings.json",
        })
    }

    pub fn prompt(self, user: &Path) -> PathBuf {
        self.home(user).join(match self {
            Self::Codex => "AGENTS.md",
            Self::Claude => "CLAUDE.md",
            Self::Gemini => "GEMINI.md",
            _ => "AGENTS.md",
        })
    }

    pub fn cli(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Dsh => "dsh",
            Self::Workbuddy => "workbuddy",
            Self::ClaudeDesktop => "Claude",
            Self::Grokbuild => "grok",
            Self::Opencode => "opencode",
            Self::Openclaw => "openclaw",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::Mcode => "mcode",
            Self::Kimi => "kimi",
            Self::Agy => "agy",
            Self::Omp => "omp",
            Self::Copilot => "copilot",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Provider {
    pub client: Client,
    pub id: String,
    pub name: String,
    settings: Value,
    official: bool,
}

#[derive(Clone)]
struct ConnectionInfo {
    endpoint: String,
    model: String,
    key: Option<String>,
    uses_auth_header: bool,
}

const CLAUDE_FIELDS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_CUSTOM_HEADERS",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
];
const GEMINI_FIELDS: &[&str] = &[
    "GOOGLE_GEMINI_BASE_URL",
    "GEMINI_API_KEY",
    "GEMINI_MODEL",
    "GEMINI_API_KEY_AUTH_MECHANISM",
];
pub(super) const LEGACY_CODEX_PROVIDER: &str = "penguinpal-direct";

fn codex_provider_signature(provider: &dyn toml_edit::TableLike) -> String {
    let mut fields: Vec<_> = provider
        .iter()
        .map(|(key, value)| (key, value.to_string().trim().to_owned()))
        .collect();
    fields.sort_unstable();
    serde_json::to_string(&fields).unwrap_or_default()
}

pub(super) fn codex_definitions(
    doc: &mut DocumentMut,
) -> Result<&mut dyn toml_edit::TableLike, String> {
    doc.entry("model_providers")
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| "model_providers 必须是 TOML 表。".into())
}

pub(super) fn register_codex_route(doc: &mut DocumentMut, route: Item) -> Result<(), String> {
    let signature =
        codex_provider_signature(route.as_table_like().ok_or("Codex 供应商定义无效。")?);
    let digest = crate::companion_sync::digest(signature.as_bytes());
    let provider_id = format!("penguinpal-{}", &digest[..24]);
    let definitions = codex_definitions(doc)?;
    if let Some(existing) = definitions.get(&provider_id) {
        if !existing
            .as_table_like()
            .is_some_and(|entry| codex_provider_signature(entry) == signature)
        {
            return Err("此中转的 Codex 供应商定义已被修改，请在客户端核对。".into());
        }
    } else {
        definitions.insert(&provider_id, route);
    }
    doc.insert("model_provider", toml_edit::value(provider_id));
    Ok(())
}

pub(super) fn toml(bytes: Option<&[u8]>) -> Result<DocumentMut, String> {
    std::str::from_utf8(bytes.unwrap_or_default())
        .map_err(|_| "TOML 不是 UTF-8。")?
        .parse()
        .map_err(|_| "配置 TOML 无效，已停止操作。".into())
}

pub(super) fn json_object(bytes: Option<&[u8]>) -> Result<Value, String> {
    let Some(bytes) = bytes else {
        return Ok(json!({}));
    };
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "配置 JSON 无效，已停止操作。")?;
    if !value.is_object() {
        return Err("配置必须是 JSON 对象。".into());
    }
    Ok(value)
}

pub(super) fn json_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| "无法编码配置。")?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn item_text<'a>(doc: &'a DocumentMut, name: &str) -> &'a str {
    doc.get(name).and_then(Item::as_str).unwrap_or_default()
}

fn object_child<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut Value, String> {
    let entry = value
        .as_object_mut()
        .ok_or("配置层级必须是对象。")?
        .entry(key)
        .or_insert_with(|| json!({}));
    if !entry.is_object() {
        return Err(format!("{key} 必须是对象。"));
    }
    Ok(entry)
}

fn env_values(bytes: Option<&[u8]>) -> Result<Value, String> {
    let source = std::str::from_utf8(bytes.unwrap_or_default()).map_err(|_| ".env 不是 UTF-8。")?;
    let mut vars = serde_json::Map::new();
    for line in source.lines() {
        if let Some((key, value)) = env_line(line) {
            if GEMINI_FIELDS.contains(&key) {
                vars.insert(key.into(), json!(value.trim_matches(['"', '\''])));
            }
        }
    }
    Ok(Value::Object(vars))
}

fn env_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_start();
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, value) = line.split_once('=')?;
    Some((key.trim_end(), value.trim()))
}

fn env_patch(bytes: Option<&[u8]>, vars: &Value, keep_model: bool) -> Result<Vec<u8>, String> {
    let source = std::str::from_utf8(bytes.unwrap_or_default()).map_err(|_| ".env 不是 UTF-8。")?;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut next: String = source
        .split_inclusive('\n')
        .filter(|line| {
            !env_line(line).is_some_and(|(key, _)| {
                GEMINI_FIELDS.contains(&key) && !(keep_model && key == "GEMINI_MODEL")
            })
        })
        .collect();
    for key in GEMINI_FIELDS {
        if keep_model && *key == "GEMINI_MODEL" {
            continue;
        }
        let Some(value) = vars[key].as_str() else {
            continue;
        };
        if value.contains(['\r', '\n', '\0', '"', '\\', '$']) {
            return Err("Gemini 参数含有不支持的 .env 字符。".into());
        }
        if !next.is_empty() && !next.ends_with('\n') {
            next.push_str(newline);
        }
        next.push_str(&format!("{key}=\"{value}\"{newline}"));
    }
    Ok(next.into_bytes())
}

pub(super) fn direct_url(value: &str) -> Result<Option<reqwest::Url>, String> {
    if value.is_empty() {
        return Ok(None);
    }
    let url = reqwest::Url::parse(value).map_err(|_| "直连地址格式无效。")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("直连地址必须是不含凭据和查询参数的 HTTP(S) 地址。".into());
    }
    let is_loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if is_loopback && url.port() == Some(15721) {
        return Err("这是 CC Switch 的本地代理地址。".into());
    }
    Ok(Some(url))
}

fn is_placeholder(value: &str) -> bool {
    value.eq_ignore_ascii_case("PROXY_MANAGED") || value.eq_ignore_ascii_case("PROXY_TOKEN")
}

impl Provider {
    pub(super) fn new_direct(
        client: Client,
        name: &str,
        endpoint: &str,
        key: &str,
        model: &str,
    ) -> Result<Self, String> {
        let (name, endpoint, key, model) = (
            name.trim(),
            endpoint.trim().trim_end_matches('/'),
            key.trim(),
            model.trim(),
        );
        let endpoint = direct_url(endpoint)?.ok_or("请输入中转 API 地址。")?;
        if name.is_empty()
            || name.chars().count() > 80
            || name.chars().any(char::is_control)
            || model.is_empty()
            || model.chars().count() > 120
            || model.chars().any(char::is_whitespace)
            || key.is_empty()
            || key.len() > 8192
            || key.chars().any(char::is_control)
            || is_placeholder(key)
        {
            return Err("请输入有效的名称、模型 ID 和 API Key。".into());
        }
        let url = endpoint.as_str().trim_end_matches('/');
        let settings = match client {
            Client::Codex => {
                let mut config = DocumentMut::new();
                config["model_provider"] = toml_edit::value("direct");
                config["model"] = toml_edit::value(model);
                config["model_providers"]["direct"]["base_url"] = toml_edit::value(url);
                config["model_providers"]["direct"]["wire_api"] = toml_edit::value("responses");
                json!({"config":config.to_string(),"auth":{"OPENAI_API_KEY":key}})
            }
            Client::Claude => {
                json!({"model":model,"env":{"ANTHROPIC_BASE_URL":url,"ANTHROPIC_AUTH_TOKEN":key}})
            }
            Client::Gemini => {
                json!({"config":{"model":{"name":model}},"env":{"GOOGLE_GEMINI_BASE_URL":url,"GEMINI_API_KEY":key}})
            }
            _ if super::native_models::supports_direct(client) => {
                json!({"direct":{"url":url,"key":key,"model":model}})
            }
            _ => return Err("当前客户端没有桌宠可写入的中转配置适配。".into()),
        };
        let provider = Self {
            client,
            id: format!("direct-{}", uuid::Uuid::new_v4().simple()),
            name: name.into(),
            settings,
            official: false,
        };
        provider.switchable()?;
        Ok(provider)
    }

    pub fn official(client: Client) -> Self {
        Self {
            client,
            id: "official".into(),
            name: match client {
                Client::Codex => "OpenAI 官方",
                Client::Claude => "Claude 官方",
                Client::Gemini => "Gemini 官方",
                other => other.label(),
            }
            .into(),
            settings: json!({}),
            official: true,
        }
    }

    fn codex(&self) -> Result<DocumentMut, String> {
        let config = self
            .settings
            .get("config")
            .and_then(Value::as_str)
            .unwrap_or_default();
        toml(Some(config.as_bytes()))
    }

    fn safe_id(&self) -> String {
        format!("{}:{}", self.client.id(), self.id)
    }

    fn codex_route(doc: &DocumentMut) -> Option<&dyn toml_edit::TableLike> {
        let id = item_text(doc, "model_provider");
        doc.get("model_providers")?
            .as_table_like()?
            .get(id)?
            .as_table_like()
    }

    fn info(&self) -> Result<ConnectionInfo, String> {
        if let Some(spec) = self.settings.get("direct") {
            let endpoint = spec["url"].as_str().ok_or("中转地址无效。")?;
            let model = spec["model"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or("模型无效。")?;
            let key = spec["key"]
                .as_str()
                .filter(|value| !value.is_empty() && !is_placeholder(value))
                .ok_or("中转 API Key 无效。")?;
            direct_url(endpoint)?.ok_or("中转地址无效。")?;
            if !super::native_models::supports_direct(self.client) {
                return Err("当前客户端没有可写入的中转配置适配。".into());
            }
            return Ok(ConnectionInfo {
                endpoint: endpoint.into(),
                model: model.into(),
                key: Some(key.into()),
                uses_auth_header: true,
            });
        }
        let vars = &self.settings["env"];
        let mut auth_header = false;
        let (endpoint, model, key) = match self.client {
            Client::Codex => {
                let doc = self.codex()?;
                let route = Self::codex_route(&doc);
                let wire = route.and_then(|r| r.get("wire_api")).and_then(Item::as_str);
                if wire.is_some_and(|wire| wire != "responses") {
                    return Err("此 Codex 供应商需要协议转换。".into());
                }
                let string = |key| {
                    route
                        .and_then(|r| r.get(key))
                        .and_then(Item::as_str)
                        .unwrap_or("")
                };
                let mut endpoint = string("base_url");
                if endpoint.is_empty() {
                    endpoint = item_text(&doc, "openai_base_url");
                }
                let env_key = string("env_key");
                let key = if env_key.is_empty() {
                    Some(string("experimental_bearer_token"))
                        .filter(|token| !token.trim().is_empty())
                        .or_else(|| {
                            self.settings["auth"]["OPENAI_API_KEY"]
                                .as_str()
                                .filter(|key| !key.trim().is_empty())
                        })
                        .map(str::to_owned)
                } else {
                    env::var(env_key).ok().filter(|key| !key.trim().is_empty())
                };
                let selected = item_text(&doc, "model_provider");
                let official_mirror =
                    matches!(selected, "custom" | super::codex_models::UNIFIED_PROVIDER)
                        && ["", "openai", "openai official", "openai 官方"]
                            .iter()
                            .any(|name| string("name").eq_ignore_ascii_case(name))
                        && route
                            .and_then(|r| r.get("requires_openai_auth"))
                            .and_then(Item::as_bool)
                            == Some(true)
                        && key.is_none()
                        && env_key.is_empty()
                        && route.is_some_and(|r| {
                            r.get("http_headers").is_none() && r.get("env_http_headers").is_none()
                        });
                if endpoint.is_empty() && !matches!(selected, "" | "openai") && !official_mirror {
                    return Err("该 Codex 配置缺少显式直连地址。".into());
                }
                if endpoint.is_empty() && key.is_some() {
                    endpoint = "https://api.openai.com/v1";
                }
                (
                    endpoint.to_owned(),
                    item_text(&doc, "model").to_owned(),
                    key,
                )
            }
            Client::Claude => {
                auth_header = vars["ANTHROPIC_AUTH_TOKEN"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty());
                (
                    vars["ANTHROPIC_BASE_URL"].as_str().unwrap_or("").to_owned(),
                    self.settings["model"]
                        .as_str()
                        .or_else(|| vars["ANTHROPIC_MODEL"].as_str())
                        .unwrap_or("")
                        .to_owned(),
                    vars[if auth_header {
                        "ANTHROPIC_AUTH_TOKEN"
                    } else {
                        "ANTHROPIC_API_KEY"
                    }]
                    .as_str()
                    .map(str::to_owned),
                )
            }
            Client::Gemini => {
                auth_header = vars["GEMINI_API_KEY_AUTH_MECHANISM"].as_str() == Some("bearer");
                (
                    vars["GOOGLE_GEMINI_BASE_URL"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                    self.settings["config"]["model"]["name"]
                        .as_str()
                        .or_else(|| vars["GEMINI_MODEL"].as_str())
                        .unwrap_or("")
                        .to_owned(),
                    vars["GEMINI_API_KEY"].as_str().map(str::to_owned),
                )
            }
            _ => return Err("此客户端由自身管理直连配置。".into()),
        };
        direct_url(&endpoint)?;
        if key.as_deref().is_some_and(is_placeholder) {
            return Err("该配置需要 CC Switch 本地代理。".into());
        }
        Ok(ConnectionInfo {
            endpoint,
            model,
            key,
            uses_auth_header: auth_header,
        })
    }

    pub fn view(&self) -> Value {
        match self.info() {
            Ok(info) => json!({"id": self.safe_id(), "name": self.name, "official": self.official,
                "model": info.model, "host": direct_url(&info.endpoint).ok().flatten()
                    .and_then(|url| url.host_str().map(str::to_owned)).unwrap_or_default()}),
            Err(_) => json!({"id": self.safe_id(), "name": self.name, "official": self.official}),
        }
    }

    fn live_native_model(&self, user: &Path) -> Option<String> {
        let info = self.info().ok()?;
        if self.client == Client::Workbuddy {
            let bytes = read_config(&self.client.config(user)).ok()??;
            let doc: Value = serde_json::from_slice(&bytes).ok()?;
            return doc
                .as_array()
                .or_else(|| doc["models"].as_array())?
                .iter()
                .find(|row| {
                    row["penguinPalProviderId"] == self.id
                        && row["id"] == info.model
                        && row["url"] == info.endpoint
                })
                .map(|_| info.model);
        }
        if matches!(self.client, Client::Hermes | Client::Mcode | Client::Omp) {
            return super::native_models::yaml_relay_model(
                self.client,
                user,
                &self.id,
                &info.model,
            );
        }
        let (selected, models) = super::native_models::catalog(self.client, user).ok()?;
        let selected = selected?;
        if matches!(self.client, Client::Kimi | Client::Grokbuild) {
            return (selected == self.id).then(|| {
                models
                    .into_iter()
                    .find(|row| row.id == selected)
                    .map_or(info.model, |row| row.model)
            });
        }
        let (provider, model) = selected.split_once('/')?;
        (provider == self.id).then(|| model.into())
    }

    pub fn switchable(&self) -> Result<(), String> {
        let info = self.info()?;
        if self.official {
            return Ok(());
        }
        if matches!(self.client, Client::Codex | Client::Claude)
            && !info.endpoint.is_empty()
            && info.key.as_deref().is_none_or(str::is_empty)
        {
            return Err("自定义地址需要此供应商自己的 API Key 或已配置的环境变量。".into());
        }
        if self.client == Client::Gemini && info.key.as_deref().is_none_or(str::is_empty) {
            return Err("Gemini 直连配置缺少 API Key。".into());
        }
        Ok(())
    }

    pub fn plan(&self, user: &Path) -> Result<Vec<Change>, String> {
        self.plan_config(user, true)
    }

    pub(super) fn plan_profile(&self, user: &Path) -> Result<Vec<Change>, String> {
        self.plan_config(user, false)
    }

    fn plan_config(&self, user: &Path, keep_model: bool) -> Result<Vec<Change>, String> {
        self.switchable()?;
        if super::native_models::supports_direct(self.client) {
            let info = self.info()?;
            return super::native_models::direct_plan(
                self.client,
                user,
                super::native_models::DirectModel {
                    id: &self.id,
                    name: &self.name,
                    url: &info.endpoint,
                    model: &info.model,
                    key: info.key.as_deref().ok_or("中转 API Key 无效。")?,
                },
            );
        }
        let path = self.client.config(user);
        let before = read_config(&path)?;
        let after = match self.client {
            Client::Codex => {
                let mut doc = toml(before.as_deref())?;
                let source = self.codex()?;
                let info = self.info()?;
                for key in ["model_provider", "openai_base_url", "chatgpt_base_url"] {
                    doc.remove(key);
                }
                for key in ["model", "model_reasoning_effort"] {
                    match (doc.get_mut(key), source.get(key)) {
                        (Some(previous), Some(value)) => {
                            let mut next = value.clone();
                            if let (Some(previous), Some(next)) =
                                (previous.as_value(), next.as_value_mut())
                            {
                                *next.decor_mut() = previous.decor().clone();
                            }
                            *previous = next;
                        }
                        (None, Some(value)) => {
                            doc.insert(key, value.clone());
                        }
                        (_, None) => {
                            doc.remove(key);
                        }
                    }
                }
                if !self.official && !info.endpoint.is_empty() {
                    let mut table = Table::new();
                    if let Some(route) = Self::codex_route(&source) {
                        for key in [
                            "name",
                            "supports_websockets",
                            "request_max_retries",
                            "stream_max_retries",
                            "stream_idle_timeout_ms",
                            "query_params",
                            "http_headers",
                            "env_http_headers",
                            "model_catalog_url",
                        ] {
                            if let Some(item) = route.get(key) {
                                table.insert(key, item.clone());
                            }
                        }
                        if let Some(item) = route.get("env_key") {
                            table.insert("env_key", item.clone());
                        }
                    }
                    table.insert("name", toml_edit::value(self.name.clone()));
                    table.insert("base_url", toml_edit::value(info.endpoint));
                    table.insert("wire_api", toml_edit::value("responses"));
                    if table.get("env_key").is_none() {
                        if let Some(key) = info.key {
                            table.insert("experimental_bearer_token", toml_edit::value(key));
                        }
                    }
                    register_codex_route(&mut doc, Item::Table(table))?;
                }
                doc.to_string().into_bytes()
            }
            Client::Claude => {
                let mut doc = json_object(before.as_deref())?;
                let has_model =
                    doc.get("model").is_some() || doc["env"].get("ANTHROPIC_MODEL").is_some();
                let vars = object_child(&mut doc, "env")?.as_object_mut().unwrap();
                for key in CLAUDE_FIELDS {
                    if keep_model && key.ends_with("_MODEL") {
                        continue;
                    }
                    vars.remove(*key);
                    if !self.official {
                        if let Some(value) = self.settings["env"][key].as_str() {
                            vars.insert((*key).into(), json!(value));
                        }
                    }
                }
                if !keep_model || !has_model {
                    doc.as_object_mut().unwrap().remove("model");
                    if let Some(model) = self.settings["model"].as_str().or_else(|| {
                        keep_model
                            .then(|| self.settings["env"]["ANTHROPIC_MODEL"].as_str())
                            .flatten()
                    }) {
                        doc["model"] = json!(model);
                    }
                }
                json_bytes(&doc)?
            }
            Client::Gemini => {
                let mut doc = json_object(before.as_deref())?;
                let auth = object_child(object_child(&mut doc, "security")?, "auth")?;
                auth["selectedType"] = json!(if self.official {
                    "oauth-personal"
                } else {
                    "gemini-api-key"
                });
                let env_path = self.client.home(user).join(".env");
                let env_before = read_config(&env_path)?;
                let env_model = env_values(env_before.as_deref())?["GEMINI_MODEL"].is_string();
                if !keep_model || (doc["model"].get("name").is_none() && !env_model) {
                    let model = object_child(&mut doc, "model")?;
                    model.as_object_mut().unwrap().remove("name");
                    if let Some(name) =
                        self.settings["config"]["model"]["name"]
                            .as_str()
                            .or_else(|| {
                                keep_model
                                    .then(|| self.settings["env"]["GEMINI_MODEL"].as_str())
                                    .flatten()
                            })
                    {
                        model["name"] = json!(name);
                    }
                }
                let vars = if self.official {
                    Value::Null
                } else {
                    self.settings["env"].clone()
                };
                let env_after = env_patch(env_before.as_deref(), &vars, keep_model)?;
                return Ok(vec![
                    Change::new(path, before, json_bytes(&doc)?),
                    Change::new(env_path, env_before, env_after),
                ]);
            }
            _ => return Err("此客户端由自身管理直连配置。".into()),
        };
        Ok(vec![Change::new(path, before, after)])
    }

    pub(super) fn codex_model_endpoint(&self) -> Result<Option<(reqwest::Url, String)>, String> {
        if self.client != Client::Codex || self.official {
            return Ok(None);
        }
        let info = self.info()?;
        let (Some(mut base), Some(key)) = (direct_url(&info.endpoint)?, info.key) else {
            return Ok(None);
        };
        let doc = self.codex()?;
        if let Some(catalog) = Self::codex_route(&doc)
            .and_then(|table| table.get("model_catalog_url"))
            .and_then(Item::as_str)
        {
            let catalog = direct_url(catalog)?.ok_or("Codex 模型目录地址无效。")?;
            if catalog.origin() != base.origin() {
                return Err("Codex 模型目录需要与中转 API 使用同一来源。".into());
            }
            return Ok(Some((catalog, key)));
        }
        base.path_segments_mut()
            .map_err(|_| "Codex 中转地址无效。")?
            .pop_if_empty()
            .push("models");
        Ok(Some((base, key)))
    }

    pub async fn probe(&self) -> Result<Value, String> {
        let info = self.info()?;
        let Some(key) = info.key.filter(|key| !key.trim().is_empty()) else {
            let result = if self.official {
                "clientLogin"
            } else {
                "noApiKey"
            };
            return Ok(json!({"result":result}));
        };
        let base = match (self.client, info.endpoint.trim_end_matches('/')) {
            (Client::Claude, "") => "https://api.anthropic.com",
            (Client::Gemini, "") => "https://generativelanguage.googleapis.com",
            (_, "") => return Err("当前配置未提供用于检查的 API 地址。".into()),
            (_, endpoint) => endpoint,
        };
        let suffix = match self.client {
            Client::Claude if !base.ends_with("/v1") => "/v1/models",
            Client::Gemini if !base.ends_with("/v1beta") && !base.ends_with("/v1") => {
                "/v1beta/models"
            }
            _ => "/models",
        };
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|_| "无法准备连接检查。")?;
        let mut request = client.get(format!("{base}{suffix}"));
        if self.client == Client::Claude {
            request = request.header("anthropic-version", "2023-06-01");
        }
        request = match self.client {
            Client::Claude if !info.uses_auth_header => request.header("x-api-key", key),
            Client::Gemini if !info.uses_auth_header => request.header("x-goog-api-key", key),
            _ => request.bearer_auth(key),
        };
        let response = request
            .send()
            .await
            .map_err(|_| "连接测试失败，请检查地址和网络。")?;
        let status = response.status();
        Ok(json!({"result":"http", "success":status.is_success(), "status":status.as_u16()}))
    }
}

pub(super) fn current(client: Client, user: &Path) -> Result<Provider, String> {
    let config = read_config(&client.config(user))?;
    let settings = match client {
        Client::Codex => {
            let doc = toml(config.as_deref())?;
            json!({ "config": doc.to_string() })
        }
        Client::Claude => json_object(config.as_deref())?,
        Client::Gemini => {
            json!({"env": env_values(read_config(&client.home(user).join(".env"))?.as_deref())?,
            "config": json_object(config.as_deref())?})
        }
        _ => return Err("此客户端由自身管理直连配置。".into()),
    };
    let mut provider = Provider {
        client,
        id: "live".into(),
        name: "当前配置".into(),
        settings,
        official: false,
    };
    if let Ok(info) = provider.info() {
        provider.official = info.endpoint.is_empty()
            && info.key.is_none()
            && (client != Client::Gemini
                || provider.settings["config"]["security"]["auth"]["selectedType"]
                    != "gemini-api-key");
    }
    Ok(provider)
}

fn same_connection(live: &ConnectionInfo, saved: &ConnectionInfo) -> bool {
    !live.endpoint.is_empty()
        && live.endpoint.trim_end_matches('/') == saved.endpoint.trim_end_matches('/')
        && live.key == saved.key
        && live.uses_auth_header == saved.uses_auth_header
}

const ORIGINAL_ID: &str = "original-local";

pub(super) fn original(client: Client, user: &Path, known: &[Provider]) -> Option<Provider> {
    if !client.manages_config()
        || !client.config(user).is_file()
        || known
            .iter()
            .any(|p| p.client == client && p.id == ORIGINAL_ID)
    {
        return None;
    }
    let mut provider = current(client, user).ok()?;
    if provider.official || provider.switchable().is_err() {
        return None;
    }
    let info = provider.info().ok()?;
    if known
        .iter()
        .any(|p| p.client == client && p.info().is_ok_and(|saved| same_connection(&info, &saved)))
    {
        return None;
    }
    provider.id = ORIGINAL_ID.into();
    provider.name = "本机原配置".into();
    Some(provider)
}

pub(super) fn list_for(client: Client, user: &Path, imported: &[Provider]) -> Value {
    if !client.manages_config() {
        let mut view = desktop_config(client, user);
        let rows = view["providers"].as_array_mut().unwrap();
        for provider in imported.iter().filter(|entry| entry.client == client) {
            let mut row = provider.view();
            row["savedDirect"] = json!(true);
            if let Some(model) = provider.live_native_model(user) {
                row["model"] = json!(model);
                if matches!(client, Client::Workbuddy | Client::Omp) {
                    row["installed"] = json!(true);
                } else {
                    row["inUse"] = json!(true);
                    for model in rows.iter_mut() {
                        if model["model"] == row["model"] && model["inUse"] == true {
                            model["inUse"] = json!(false);
                        }
                    }
                }
            }
            rows.push(row);
        }
        if let Some(active) = rows.iter().rev().find(|row| row["inUse"] == true).cloned() {
            view["current"] = active;
        }
        return view;
    }
    let official = Provider::official(client);
    let available: Vec<_> = imported
        .iter()
        .filter(|provider| provider.client == client)
        .filter(|provider| {
            provider.id != ORIGINAL_ID
                || !provider.info().is_ok_and(|original| {
                    imported.iter().any(|saved| {
                        saved.client == client
                            && saved.id != ORIGINAL_ID
                            && saved
                                .info()
                                .is_ok_and(|info| same_connection(&original, &info))
                    })
                })
        })
        .collect();
    let mut choices = vec![official.view()];
    choices.extend(available.iter().map(|provider| provider.view()));
    let active = current(client, user).and_then(|provider| Ok((provider.info()?, provider)));
    let active_view = active.as_ref().ok().map(|(info, provider)| {
        let mut view = provider.view();
        let inferred_name = (client == Client::Codex)
            .then(|| provider.codex().ok())
            .flatten()
            .and_then(|doc| {
                Provider::codex_route(&doc)
                    .and_then(|route| route.get("name"))
                    .and_then(Item::as_str)
                    .map(|name| name.chars().take(120).collect::<String>())
            });
        let matches = |candidate: &&Provider| {
            candidate.info().is_ok_and(|candidate| {
                !info.endpoint.is_empty()
                    && candidate.endpoint.trim_end_matches('/')
                        == info.endpoint.trim_end_matches('/')
                    && (candidate.key == info.key || info.key.is_none())
                    && candidate.uses_auth_header == info.uses_auth_header
            })
        };
        let selected = if provider.official {
            Some(&official)
        } else {
            let mut matching = available.iter().copied().filter(matches);
            let first = matching.next();
            if matching.next().is_none() {
                first
            } else {
                available
                    .iter()
                    .copied()
                    .filter(matches)
                    .find(|p| inferred_name.as_deref() == Some(p.name.as_str()))
            }
        };
        if let Some(selected) = selected {
            view["name"] = json!(selected.name);
            view["id"] = json!(selected.safe_id());
            if let Some(row) = choices
                .iter_mut()
                .find(|row| row["id"] == selected.safe_id())
            {
                row["inUse"] = json!(client.home(user).exists());
                row["model"] = json!(info.model);
            }
        } else {
            view["name"] = json!(inferred_name.unwrap_or_else(|| "本机当前配置".into()));
            let mut local = view.clone();
            local["inUse"] = json!(true);
            choices.insert(0, local);
        }
        view
    });
    let unified_history = client == Client::Codex
        && active.as_ref().is_ok_and(|(_, provider)| {
            provider
                .codex()
                .is_ok_and(|doc| super::codex_models::unified(&doc))
        });
    let configured = client.home(user).exists();
    let configured_model = if client == Client::Codex && active_view.is_none() {
        read_config(&client.config(user))
            .ok()
            .flatten()
            .and_then(|bytes| toml(Some(&bytes)).ok())
            .map(|doc| item_text(&doc, "model").to_owned())
    } else {
        None
    };
    json!({
        "id": client, "label": client.label(), "configured": configured,
        "config": client.config(user),
        "current": active_view,
        "configuredModel": configured_model,
        "unifiedHistory": unified_history,
        "error": active.err(),
        "providers": choices,
    })
}

fn desktop_config(client: Client, user: &Path) -> Value {
    let path = client.config(user);
    let configured = client.home(user).is_dir();
    let mut providers = Vec::new();
    if client == Client::Workbuddy {
        let data = read_config(&path)
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let models = data
            .as_ref()
            .and_then(|data| data.as_array().or_else(|| data["models"].as_array()));
        for (index, model) in models.into_iter().flatten().take(100).enumerate() {
            if model.get("penguinPalProviderId").is_some() {
                continue;
            }
            let Some(id) = model["id"].as_str() else {
                continue;
            };
            let name = model["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .unwrap_or(id);
            let host = model["url"]
                .as_str()
                .and_then(|url| direct_url(url).ok().flatten())
                .and_then(|url| url.host_str().map(str::to_owned));
            providers.push(
                json!({"id":format!("workbuddy:{index}"),"name":name,"model":id,"host":host}),
            );
        }
    }
    let catalog = super::native_models::catalog(client, user);
    let error = catalog.as_ref().err().cloned();
    let (model, models) = catalog.unwrap_or_default();
    for choice in models {
        providers.push(json!({"id":choice.id,"name":choice.name,"model":choice.model,
            "host":choice.host,"inUse":model.as_deref() == Some(choice.id.as_str()),"modelChoice":choice.selectable}));
    }
    let current = providers.iter().find(|row| row["inUse"] == true);
    json!({"id":client,"label":client.label(),"config":path,"configured":configured,
        "managedByClient":true,"sessionLogs":client.has_session_logs(),"providers":providers,
        "configuredModel": model, "current": current, "error":error})
}

pub(super) fn find(id: &str, client: Client, imported: &[Provider]) -> Result<Provider, String> {
    let official = Provider::official(client);
    if official.safe_id() == id {
        return Ok(official);
    }
    imported
        .iter()
        .find(|p| p.client == client && p.safe_id() == id)
        .cloned()
        .ok_or_else(|| "供应商不在现有直连列表中。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_relay_is_hidden_only_when_a_saved_relay_matches() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path();
        let path = Client::Codex.config(user);
        assert!(path.starts_with(user));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let saved = Provider::new_direct(
            Client::Codex,
            "Saved relay",
            "https://fixture.example/v1",
            "fixture-key",
            "old-model",
        )
        .unwrap();
        let live = Provider::new_direct(
            Client::Codex,
            "Live relay",
            "https://fixture.example/v1",
            "fixture-key",
            "live-model",
        )
        .unwrap();
        let write_live = |provider: &Provider| {
            let changes = provider.plan(user).unwrap();
            std::fs::write(&path, changes[0].after.as_ref().unwrap()).unwrap();
        };
        write_live(&live);
        let original = original(Client::Codex, user, &[]).unwrap();
        assert!(super::original(Client::Codex, user, std::slice::from_ref(&saved)).is_none());

        let list = list_for(Client::Codex, user, &[saved.clone(), original.clone()]);
        assert_eq!(list["providers"].as_array().unwrap().len(), 2);
        assert_eq!(list["current"]["name"], "Saved relay");
        assert_eq!(list["current"]["model"], "live-model");

        let other_key = Provider::new_direct(
            Client::Codex,
            "Other account",
            "https://fixture.example/v1",
            "other-fixture-key",
            "old-model",
        )
        .unwrap();
        let list = list_for(Client::Codex, user, &[other_key, original]);
        assert_eq!(list["providers"].as_array().unwrap().len(), 3);
        assert_eq!(list["current"]["name"], "本机原配置");
    }

    #[test]
    fn provider_checks_skip_anonymous_requests() {
        let official = Provider::official(Client::Codex);
        let official_result = tauri::async_runtime::block_on(official.probe()).unwrap();
        assert_eq!(official_result["result"], "clientLogin");

        let direct = Provider {
            client: Client::Codex,
            id: "fixture".into(),
            name: "Fixture".into(),
            settings: json!({"config": "openai_base_url = 'https://fixture.example/v1'"}),
            official: false,
        };
        let direct_result = tauri::async_runtime::block_on(direct.probe()).unwrap();
        assert_eq!(direct_result["result"], "noApiKey");
    }

    #[test]
    fn official_codex_alias_and_inline_direct_provider_are_recognized() {
        let dir = tempfile::tempdir().unwrap();
        let path = Client::Codex.config(dir.path());
        assert!(path.starts_with(dir.path()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "model = 'fixture-model'\nmodel_provider = 'custom'\n[model_providers]\ncustom = {name = 'OpenAI Official', requires_openai_auth = true, wire_api = 'responses'}\n",
        ).unwrap();
        let view = list_for(Client::Codex, dir.path(), &[]);
        assert_eq!(view["current"]["model"], "fixture-model");
        assert_eq!(view["providers"][0]["inUse"], true);

        std::fs::write(
            &path,
            "model = 'fixture-model'\nmodel_provider = 'custom'\n[model_providers]\ncustom = {name = 'Direct', base_url = 'https://direct.example/v1/', wire_api = 'responses'}\n",
        ).unwrap();
        let view = list_for(Client::Codex, dir.path(), &[]);
        assert_eq!(view["current"]["name"], "Direct");
        assert_eq!(view["current"]["host"], "direct.example");
        assert_eq!(view["providers"][0]["inUse"], true);
    }

    #[test]
    fn codex_switch_preserves_other_routes_mcp_and_auth_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = Client::Codex.home(dir.path());
        assert!(
            home.starts_with(dir.path()),
            "run with fixture CODEX_HOME unset"
        );
        std::fs::create_dir_all(&home).unwrap();
        let file = home.join("config.toml");
        std::fs::write(&file, "# own\nmodel = 'old'\n[mcp_servers.dev]\ncommand = 'test'\n[model_providers.earlier]\nbase_url = 'https://elsewhere.example'\n").unwrap();
        std::fs::write(home.join("auth.json"), b"fixture OAuth token").unwrap();
        let provider = Provider {
            client: Client::Codex,
            id: "example".into(),
            name: "Example".into(),
            official: false,
            settings: json!({"auth":{"OPENAI_API_KEY":"fixture-key"}, "config":"model='new'\nmodel_provider='custom'\n[model_providers.custom]\nbase_url='https://direct.example/v1'\nwire_api='responses'\nrequires_openai_auth=true\n"}),
        };
        let plan = provider.plan(dir.path()).unwrap();
        let next = String::from_utf8(plan[0].after.clone().unwrap()).unwrap();
        assert!(next.contains("# own"));
        assert!(next.contains("[mcp_servers.dev]"));
        assert!(next.contains("[model_providers.earlier]"));
        assert!(next.contains("experimental_bearer_token = \"fixture-key\""));
        assert!(!next.contains("requires_openai_auth"));
        assert_eq!(
            std::fs::read(home.join("auth.json")).unwrap(),
            b"fixture OAuth token"
        );
        std::fs::write(&file, &next).unwrap();
        let imported = [provider];
        let after = list_for(Client::Codex, dir.path(), &imported);
        assert_eq!(after["current"]["name"], "Example");
        assert_eq!(after["providers"][1]["inUse"], true);
        assert_ne!(after["providers"][0]["inUse"], true);

        let restore = Provider::official(Client::Codex).plan(dir.path()).unwrap();
        std::fs::write(&file, restore[0].after.as_ref().unwrap()).unwrap();
        let after = list_for(Client::Codex, dir.path(), &imported);
        assert_eq!(after["current"]["name"], "OpenAI 官方");
        assert_eq!(after["providers"][0]["inUse"], true);
        assert_ne!(after["providers"][1]["inUse"], true);
    }

    #[test]
    fn claude_and_gemini_switch_connections_without_resetting_model_settings() {
        for client in [Client::Claude, Client::Gemini] {
            let temp = tempfile::tempdir().unwrap();
            let path = client.config(temp.path());
            assert!(path.starts_with(temp.path()));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let live = if client == Client::Claude {
                json!({"model":"new-model","env":{"ANTHROPIC_MODEL":"new-env","ANTHROPIC_DEFAULT_SONNET_MODEL":"new-sonnet"},"effortLevel":"high"})
            } else {
                json!({"model":{"name":"new-model","custom":true},"modelConfigs":{"aliases":{"latest":{"model":"new-model"}}}})
            };
            std::fs::write(&path, json_bytes(&live).unwrap()).unwrap();
            let env_path = client.home(temp.path()).join(".env");
            if client == Client::Gemini {
                std::fs::write(&env_path, "export GEMINI_MODEL='new-env'\n").unwrap();
            }
            let relay = Provider::new_direct(
                client,
                "Fixture",
                "https://fixture.example/v1",
                "fixture-key",
                "stale",
            )
            .unwrap();
            for provider in [relay.clone(), Provider::official(client)] {
                for change in provider.plan(temp.path()).unwrap() {
                    let output = change.after.unwrap();
                    assert!(!String::from_utf8_lossy(&output).contains("stale"));
                    std::fs::write(change.path, output).unwrap();
                }
                let current = json_object(read_config(&path).unwrap().as_deref()).unwrap();
                assert_eq!(current["model"], live["model"]);
                if client == Client::Claude {
                    assert_eq!(
                        current["env"]["ANTHROPIC_MODEL"],
                        live["env"]["ANTHROPIC_MODEL"]
                    );
                    assert_eq!(
                        current["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
                        live["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"]
                    );
                    assert_eq!(current["effortLevel"], "high");
                } else {
                    assert_eq!(current["modelConfigs"], live["modelConfigs"]);
                    assert!(std::fs::read_to_string(&env_path)
                        .unwrap()
                        .contains("export GEMINI_MODEL='new-env'\n"));
                }
                let list = list_for(client, temp.path(), std::slice::from_ref(&relay));
                assert_eq!(list["current"]["model"], "new-model");
                assert_eq!(list["current"]["official"], provider.official);
                assert!(list["providers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["inUse"] == true && row["model"] == "new-model"));
            }
        }
    }

    #[test]
    fn credentials_and_proxy_placeholder_are_not_public_or_switchable() {
        let provider = Provider {
            client: Client::Gemini,
            id: "example".into(),
            name: "Example".into(),
            official: false,
            settings: json!({"env": {"GEMINI_API_KEY": "PROXY_MANAGED"}}),
        };
        assert!(provider.switchable().is_err());
        assert!(!provider.view().to_string().contains("PROXY_MANAGED"));
    }
}

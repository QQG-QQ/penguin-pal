use std::path::Path;

use serde_json::{json, Map, Value};
use toml_edit::Item;

use super::{
    providers::{json_bytes, json_object, toml, Client},
    storage::{read_config, Change},
};

pub(super) struct Model {
    pub id: String,
    pub model: String,
    pub name: String,
    pub host: Option<String>,
    pub selectable: bool,
}

pub(super) fn supports_direct(client: Client) -> bool {
    matches!(
        client,
        Client::Workbuddy
            | Client::Pi
            | Client::Opencode
            | Client::Openclaw
            | Client::Dsh
            | Client::Grokbuild
            | Client::Kimi
            | Client::Copilot
            | Client::Hermes
            | Client::Mcode
            | Client::Omp
    )
}

pub(super) struct DirectModel<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub url: &'a str,
    pub model: &'a str,
    pub key: &'a str,
}

pub(super) fn direct_plan(
    client: Client,
    user: &Path,
    direct: DirectModel<'_>,
) -> Result<Vec<Change>, String> {
    let DirectModel {
        id,
        name,
        url,
        model,
        key,
    } = direct;
    if !id.starts_with("direct-") || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return Err("中转编号无效。".into());
    }
    if client == Client::Dsh {
        return super::mcp_yaml::dsh_direct_plan(user, direct);
    }
    if matches!(client, Client::Hermes | Client::Mcode | Client::Omp) {
        return yaml_provider_plan(client, user, direct);
    }
    let path = client.config(user);
    let before = read_config(&path)?;
    if matches!(client, Client::Grokbuild | Client::Kimi) {
        let mut config = toml(before.as_deref())?;
        let (selected, entries) = toml_models(client, &config);
        let model = selected
            .and_then(Item::as_str)
            .map(|alias| {
                entries
                    .and_then(|table| table.get(alias))
                    .and_then(|entry| entry.get("model"))
                    .and_then(Item::as_str)
                    .unwrap_or(alias)
            })
            .filter(|model| !model.is_empty())
            .unwrap_or(model)
            .to_owned();
        if client == Client::Grokbuild {
            let mut entry = toml_edit::Table::new();
            for (field, value) in [
                ("name", name),
                ("model", model.as_str()),
                ("base_url", url),
                ("api_key", key),
                ("api_backend", "responses"),
            ] {
                entry.insert(field, toml_edit::value(value));
            }
            let entry = merge_toml_entry(toml_table(&mut config, "model")?, id, entry)?;
            if !entry.contains_key("context_window") {
                entry.insert("context_window", toml_edit::value(128_000));
            }
            toml_table(&mut config, "models")?.insert("default", toml_edit::value(id));
        } else {
            let mut provider = toml_edit::Table::new();
            for (field, value) in [("type", "openai"), ("base_url", url), ("api_key", key)] {
                provider.insert(field, toml_edit::value(value));
            }
            let mut entry = toml_edit::Table::new();
            entry.insert("model", toml_edit::value(model));
            entry.insert("provider", toml_edit::value(id));
            merge_toml_entry(toml_table(&mut config, "providers")?, id, provider)?;
            let entry = merge_toml_entry(toml_table(&mut config, "models")?, id, entry)?;
            if !entry.contains_key("max_context_size") {
                entry.insert("max_context_size", toml_edit::value(128_000));
            }
            config.insert("default_model", toml_edit::value(id));
        }
        return Ok(vec![Change::new(
            path,
            before,
            config.to_string().into_bytes(),
        )]);
    }
    if client == Client::Copilot {
        let mut settings = json_object(before.as_deref())?;
        let model = settings["model"]
            .as_str()
            .and_then(model_suffix)
            .unwrap_or(model)
            .to_owned();
        let provider_path = std::env::var_os("COPILOT_PROVIDERS_CONFIG")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| client.home(user).join("providers.json"));
        let raw = read_config(&provider_path)?;
        let mut listing = json_object(raw.as_deref())?;
        let providers = listing
            .as_object_mut()
            .unwrap()
            .entry("providers")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("Copilot 的供应商列表格式无效。")?;
        let entry =
            json!({"name":id,"type":"openai","wireApi":"completions","baseUrl":url,"apiKey":key});
        if let Some(old) = providers.iter_mut().find(|old| old["name"] == id) {
            merge_provider(old, entry)?;
        } else {
            providers.push(entry);
        }
        let models = listing
            .as_object_mut()
            .unwrap()
            .entry("models")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("Copilot 的自定义模型列表格式无效。")?;
        let entry = json!({"provider":id,"id":model,"name":model});
        if !models
            .iter()
            .any(|old| old["provider"] == id && old["id"] == model)
        {
            models.push(entry);
        }
        settings["model"] = json!(format!("{id}/{model}"));
        return Ok(vec![
            Change::new(provider_path, raw, json_bytes(&listing)?),
            Change::new(path, before, json_bytes(&settings)?),
        ]);
    }
    let after = if client == Client::Workbuddy {
        let raw: Value = before
            .as_deref()
            .filter(|bytes| !bytes.is_empty())
            .map(|bytes| {
                serde_json::from_slice(bytes).map_err(|_| "WorkBuddy 的 models.json 格式无效。")
            })
            .transpose()?
            .unwrap_or_else(|| json!({}));
        let mut config = if raw.is_array() {
            json!({"models":raw})
        } else {
            raw
        };
        let object = config
            .as_object_mut()
            .ok_or("WorkBuddy 的 models.json 必须是对象或列表。")?;
        let models = object
            .entry("models")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("WorkBuddy 模型列表无效。")?;
        let matching = models.iter().position(|entry| entry["id"] == model);
        let next = json!({"id":model,"name":name,"vendor":name,"url":url,"apiKey":key,"penguinPalProviderId":id});
        if let Some(index) = matching {
            if models[index]["penguinPalProviderId"] != id {
                return Err("WorkBuddy 已有同名模型 ID。".into());
            }
            merge_provider(&mut models[index], json!({"url":url,"apiKey":key}))?;
        } else {
            models.push(next);
        }
        let available = object
            .entry("availableModels")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("WorkBuddy 可用模型列表无效。")?;
        if !available.iter().any(|entry| entry == model) {
            available.push(json!(model));
        }
        json_bytes(&config)?
    } else {
        let mut document = json_object(before.as_deref())?;
        match client {
            Client::Pi => {
                let selection = client.home(user).join("settings.json");
                let previous = read_config(&selection)?;
                let mut settings = json_object(previous.as_deref())?;
                let model = settings["defaultModel"]
                    .as_str()
                    .filter(|model| !model.is_empty())
                    .unwrap_or(model);
                insert_provider(
                    &mut document,
                    "providers",
                    id,
                    json!({
                        "name":name,"baseUrl":url,"apiKey":key,"api":"openai-completions","models":[{"id":model,"name":model}]
                    }),
                )?;
                settings["defaultModel"] = json!(model);
                settings["defaultProvider"] = json!(id);
                return Ok(vec![
                    Change::new(path, before, json_bytes(&document)?),
                    Change::new(selection, previous, json_bytes(&settings)?),
                ]);
            }
            Client::Opencode => {
                let model = document["model"]
                    .as_str()
                    .and_then(model_suffix)
                    .unwrap_or(model)
                    .to_owned();
                let legacy = document.get("provider").is_some();
                if legacy && document.get("providers").is_some() {
                    return Err(
                        "OpenCode 同时存在两种供应商配置格式，请先在客户端确认所用格式。".into(),
                    );
                }
                let mut models = Map::new();
                models.insert(model.clone(), json!({"name":model}));
                if !legacy {
                    insert_provider(
                        &mut document,
                        "providers",
                        id,
                        json!({"name":name,"package":"@opencode/ai/providers/openai-compatible","settings":{"baseURL":url,"apiKey":key},"models":models}),
                    )?;
                } else {
                    insert_provider(
                        &mut document,
                        "provider",
                        id,
                        json!({"name":name,"npm":"@ai-sdk/openai-compatible","options":{"baseURL":url,"apiKey":key},"models":models}),
                    )?;
                }
                document["model"] = json!(format!("{id}/{model}"));
            }
            Client::Openclaw => {
                let model = claw_model(&document)
                    .and_then(model_suffix)
                    .unwrap_or(model)
                    .to_owned();
                let reference = format!("{id}/{model}");
                let root = object_entry(&mut document, "models")?;
                insert_provider(
                    root,
                    "providers",
                    id,
                    json!({"baseUrl":url,"apiKey":key,"api":"openai-completions","models":[{"id":model,"name":name}]}),
                )?;
                let agents = object_entry(&mut document, "agents")?;
                let defaults = object_entry(agents, "defaults")?;
                let defaults = defaults.as_object_mut().ok_or("OpenClaw 默认配置无效。")?;
                let value = defaults.entry("model").or_insert_with(|| json!({}));
                if value.is_string() {
                    *value = json!({"primary":reference});
                } else {
                    value
                        .as_object_mut()
                        .ok_or("OpenClaw 默认模型格式无效。")?
                        .insert("primary".into(), json!(reference));
                }
            }
            _ => return Err("当前客户端没有桌宠可写入的中转配置适配。".into()),
        }
        json_bytes(&document)?
    };
    Ok(vec![Change::new(path, before, after)])
}

fn yaml_provider_plan(
    client: Client,
    user: &Path,
    direct: DirectModel<'_>,
) -> Result<Vec<Change>, String> {
    let DirectModel {
        id,
        name,
        url,
        model,
        key,
    } = direct;
    let path = client.config(user);
    let before = read_config(&path)?;
    let text = std::str::from_utf8(before.as_deref().unwrap_or_default())
        .map_err(|_| "客户端 YAML 配置不是 UTF-8。")?;
    let original: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|_| "客户端现有 YAML 无法读取。")?;
    let mut config = original.clone();
    if config.is_null() {
        config = serde_yaml::Value::Mapping(Default::default());
    }
    let model = match client {
        Client::Hermes => config["model"]
            .as_str()
            .or_else(|| config["model"]["default"].as_str()),
        Client::Mcode => config["defaultModel"].as_str().and_then(model_suffix),
        _ => None,
    }
    .filter(|value| !value.is_empty())
    .unwrap_or(model)
    .to_owned();
    let root = config
        .as_mapping_mut()
        .ok_or("客户端 YAML 配置必须是对象。")?;
    let field = match client {
        Client::Hermes => "custom_providers",
        Client::Mcode => "custom_provider",
        _ => "providers",
    };
    let initial = if client == Client::Hermes {
        serde_yaml::Value::Sequence(Vec::new())
    } else {
        serde_yaml::Value::Mapping(Default::default())
    };
    let entries = root
        .entry(serde_yaml::Value::from(field))
        .or_insert(initial);
    let payload = match client {
        Client::Hermes => json!({"name":id,"base_url":url,"api_key":key,"model":model}),
        Client::Mcode => {
            json!({"name":name,"kind":"custom","api":"openai-completions","options":{"baseURL":url,"apiKey":key},"models":{(model.as_str()):{"name":model}}})
        }
        _ => {
            json!({"baseUrl":url,"apiKey":key,"api":"openai-completions","models":[{"id":model,"name":model}]})
        }
    };
    let payload = serde_yaml::to_value(payload).map_err(|_| "无法编码供应商配置。")?;
    if client == Client::Hermes {
        let list = entries
            .as_sequence_mut()
            .ok_or("Hermes 自定义供应商必须是列表。")?;
        if let Some(existing) = list
            .iter_mut()
            .find(|entry| entry["name"].as_str() == Some(id))
        {
            existing
                .as_mapping_mut()
                .ok_or("Hermes 供应商配置无效。")?
                .extend(payload.as_mapping().unwrap().clone());
        } else {
            list.push(payload);
        }
        let selected = root
            .entry(serde_yaml::Value::from("model"))
            .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));
        if selected.is_string() {
            *selected = serde_yaml::Value::Mapping(Default::default());
        }
        let selected = selected
            .as_mapping_mut()
            .ok_or("Hermes 默认模型的配置无效。")?;
        selected.insert(
            serde_yaml::Value::from("provider"),
            serde_yaml::Value::from(id),
        );
        selected.insert(
            serde_yaml::Value::from("default"),
            serde_yaml::Value::from(model),
        );
        selected.insert(
            serde_yaml::Value::from("base_url"),
            serde_yaml::Value::from(url),
        );
    } else {
        let existing = entries
            .as_mapping_mut()
            .ok_or("现有供应商配置必须是对象。")?
            .entry(serde_yaml::Value::from(id))
            .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));
        let mut merged = serde_json::to_value(&*existing).map_err(|_| "供应商配置无法转换。")?;
        merge_provider(
            &mut merged,
            serde_json::to_value(payload).map_err(|_| "供应商配置无法转换。")?,
        )?;
        *existing = serde_yaml::to_value(merged).map_err(|_| "供应商配置无法写入。")?;
        if client == Client::Mcode {
            root.insert(
                serde_yaml::Value::from("defaultModel"),
                serde_yaml::Value::from(format!("custom_provider:{id}/{model}")),
            );
        }
    }
    let fields: &[&str] = match client {
        Client::Hermes => &[field, "model"],
        Client::Mcode => &[field, "defaultModel"],
        _ => &[field],
    };
    let output = super::mcp_yaml::replace_yaml_fields(text, &original, &config, fields)?;
    Ok(vec![Change::new(path, before, output)])
}

pub(super) fn yaml_relay_model(
    client: Client,
    user: &Path,
    id: &str,
    fallback: &str,
) -> Option<String> {
    let bytes = read_config(&client.config(user)).ok()??;
    let doc: serde_yaml::Value = serde_yaml::from_slice(&bytes).ok()?;
    let model = match client {
        Client::Hermes if doc["model"]["provider"].as_str() == Some(id) => {
            doc["model"]["default"].as_str()?
        }
        Client::Mcode => {
            let (provider, model) = doc["defaultModel"].as_str()?.split_once('/')?;
            if provider.strip_prefix("custom_provider:") != Some(id) {
                return None;
            }
            model
        }
        Client::Omp
            if doc["providers"][id]["models"]
                .as_sequence()?
                .iter()
                .any(|entry| entry["id"].as_str() == Some(fallback)) =>
        {
            fallback
        }
        _ => return None,
    };
    Some(model.into())
}

fn toml_table<'a>(
    document: &'a mut toml_edit::DocumentMut,
    key: &str,
) -> Result<&'a mut toml_edit::Table, String> {
    document
        .entry(key)
        .or_insert(Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("客户端配置的 {key} 必须是 TOML 表。"))
}

fn object_entry<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut Value, String> {
    let entry = value
        .as_object_mut()
        .ok_or("客户端配置格式无效。")?
        .entry(key)
        .or_insert_with(|| json!({}));
    if !entry.is_object() {
        return Err(format!("客户端配置的 {key} 必须是对象。"));
    }
    Ok(entry)
}

pub(super) fn merge_provider(target: &mut Value, fields: Value) -> Result<(), String> {
    let target = target.as_object_mut().ok_or("现有供应商记录必须是对象。")?;
    for (key, value) in fields.as_object().unwrap() {
        match (target.get_mut(key), value) {
            (Some(Value::Object(current)), Value::Object(fields)) if key == "models" => {
                for (id, value) in fields {
                    current.entry(id).or_insert_with(|| value.clone());
                }
            }
            (Some(Value::Object(current)), Value::Object(fields)) => current.extend(fields.clone()),
            (Some(Value::Array(current)), Value::Array(models)) if key == "models" => {
                for model in models {
                    if !current.iter().any(|entry| entry["id"] == model["id"]) {
                        current.push(model.clone());
                    }
                }
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(())
}

fn insert_provider(
    document: &mut Value,
    field: &str,
    id: &str,
    provider: Value,
) -> Result<(), String> {
    let existing = object_entry(document, field)?
        .as_object_mut()
        .unwrap()
        .entry(id)
        .or_insert_with(|| json!({}));
    merge_provider(existing, provider)
}

fn merge_toml_entry<'a>(
    table: &'a mut toml_edit::Table,
    id: &str,
    fields: toml_edit::Table,
) -> Result<&'a mut dyn toml_edit::TableLike, String> {
    let entry = table
        .entry(id)
        .or_insert_with(|| Item::Table(toml_edit::Table::new()))
        .as_table_like_mut()
        .ok_or("现有供应商记录必须是 TOML 表。")?;
    for (key, value) in fields.iter() {
        entry.insert(key, value.clone());
    }
    Ok(entry)
}

fn toml_models(client: Client, doc: &toml_edit::DocumentMut) -> (Option<&Item>, Option<&Item>) {
    if client == Client::Grokbuild {
        (
            doc.get("models").and_then(|table| table.get("default")),
            doc.get("model"),
        )
    } else {
        (doc.get("default_model"), doc.get("models"))
    }
}

fn model_suffix(reference: &str) -> Option<&str> {
    let model = reference
        .split_once('/')
        .map_or(reference, |(_, model)| model);
    (!model.is_empty()).then_some(model)
}

fn claw_model(document: &Value) -> Option<&str> {
    let value = document.pointer("/agents/defaults/model")?;
    value.as_str().or_else(|| value["primary"].as_str())
}

fn host(url: &Value) -> Option<String> {
    let url = reqwest::Url::parse(url.as_str()?).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url.host_str()?.to_owned())
}

fn json_choices(client: Client, models: &Value) -> Vec<Model> {
    models
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(provider, spec)| {
            let name = spec["name"].as_str().unwrap_or(provider);
            let keys: Vec<&str> = match &spec["models"] {
                Value::Object(models) => models.keys().map(String::as_str).collect(),
                Value::Array(models) => {
                    models.iter().filter_map(|row| row["id"].as_str()).collect()
                }
                _ => Vec::new(),
            };
            keys.into_iter()
                .filter(|key| !key.is_empty())
                .map(move |key| Model {
                    id: format!("{provider}/{key}"),
                    model: key.to_owned(),
                    name: name.to_owned(),
                    selectable: true,
                    host: host(match client {
                        Client::Opencode if spec.get("settings").is_some() => {
                            &spec["settings"]["baseURL"]
                        }
                        Client::Opencode => &spec["options"]["baseURL"],
                        _ => &spec["baseUrl"],
                    }),
                })
        })
        .take(200)
        .collect()
}

pub(super) fn catalog(client: Client, user: &Path) -> Result<(Option<String>, Vec<Model>), String> {
    let path = client.config(user);
    match client {
        Client::Pi | Client::Opencode | Client::Openclaw => {
            let document = json_object(read_config(&path)?.as_deref())?;
            let (current, providers) = match client {
                Client::Pi => {
                    let settings = json_object(
                        read_config(&client.home(user).join("settings.json"))?.as_deref(),
                    )?;
                    let current = settings["defaultProvider"]
                        .as_str()
                        .zip(settings["defaultModel"].as_str())
                        .map(|(provider, model)| format!("{provider}/{model}"));
                    (current, &document["providers"])
                }
                Client::Opencode => (
                    document["model"].as_str().map(str::to_owned),
                    document.get("providers").unwrap_or(&document["provider"]),
                ),
                _ => (
                    claw_model(&document).map(str::to_owned),
                    &document["models"]["providers"],
                ),
            };
            Ok((current, json_choices(client, providers)))
        }
        Client::Grokbuild | Client::Kimi => {
            let document = toml(read_config(&path)?.as_deref())?;
            let (default, models) = toml_models(client, &document);
            let options = models
                .and_then(Item::as_table_like)
                .into_iter()
                .flat_map(|table| table.iter())
                .filter_map(|(id, row)| {
                    let model = row.get("model")?.as_str()?;
                    Some(Model {
                        id: id.to_owned(),
                        model: model.to_owned(),
                        name: row
                            .get("name")
                            .or_else(|| row.get("provider"))
                            .and_then(Item::as_str)
                            .unwrap_or(id)
                            .to_owned(),
                        host: row
                            .get("base_url")
                            .and_then(Item::as_str)
                            .and_then(|url| host(&json!(url))),
                        selectable: true,
                    })
                })
                .take(200)
                .collect();
            Ok((default.and_then(Item::as_str).map(str::to_owned), options))
        }
        Client::Copilot => {
            let document = json_object(read_config(&path)?.as_deref())?;
            Ok((document["model"].as_str().map(str::to_owned), Vec::new()))
        }
        Client::Dsh => {
            let mut current = None;
            let desktop = client.home(user).join("profiles/desktop/cordis.patch.yml");
            let global = client.home(user).join("cordis.patch.yml");
            for path in [desktop, global] {
                let Some(bytes) = read_config(&path)? else {
                    continue;
                };
                let Ok(serde_yaml::Value::Sequence(plugins)) = serde_yaml::from_slice(&bytes)
                else {
                    continue;
                };
                for plugin in plugins {
                    if plugin["id"].as_str() != Some("agent-default-model") {
                        continue;
                    }
                    let config = &plugin["config"];
                    if let Some((provider, model)) =
                        config["provider"].as_str().zip(config["model"].as_str())
                    {
                        current = Some(Model {
                            id: format!("{provider}/{model}"),
                            model: model.to_owned(),
                            name: if provider == "deepseek-account" {
                                "DeepSeek 官方".into()
                            } else {
                                provider.to_owned()
                            },
                            host: None,
                            selectable: false,
                        });
                    }
                }
            }
            Ok((
                current.as_ref().map(|model| model.id.clone()),
                current.into_iter().collect(),
            ))
        }
        _ => Ok((None, Vec::new())),
    }
}

pub(super) fn select(client: Client, user: &Path, id: &str) -> Result<Vec<Change>, String> {
    let (active, choices) = catalog(client, user)?;
    let selected = choices
        .iter()
        .find(|choice| choice.id == id && choice.selectable)
        .ok_or("此模型已不在客户端的现有配置中。")?;
    if active.as_deref() == Some(id) {
        return Ok(Vec::new());
    }
    let path = if client == Client::Pi {
        client.home(user).join("settings.json")
    } else {
        client.config(user)
    };
    let before = read_config(&path)?;
    let after = match client {
        Client::Grokbuild | Client::Kimi => {
            let mut document = toml(before.as_deref())?;
            if client == Client::Grokbuild {
                let table = document
                    .get_mut("models")
                    .and_then(Item::as_table_like_mut)
                    .ok_or("找不到 Grok Build 的模型选择表。")?;
                table.insert("default", toml_edit::value(id));
            } else {
                document["default_model"] = toml_edit::value(id);
            }
            document.to_string().into_bytes()
        }
        Client::Pi | Client::Opencode | Client::Openclaw => {
            let mut document = json_object(before.as_deref())?;
            match client {
                Client::Pi => {
                    let provider = id
                        .strip_suffix(&format!("/{}", selected.model))
                        .ok_or("Pi 模型编号无效。")?;
                    document["defaultProvider"] = json!(provider);
                    document["defaultModel"] = json!(selected.model);
                }
                Client::Opencode => document["model"] = json!(id),
                _ => {
                    let model = document
                        .pointer_mut("/agents/defaults/model")
                        .and_then(Value::as_object_mut)
                        .ok_or("当前 OpenClaw 配置没有可编辑的默认模型字段。")?;
                    model.insert("primary".into(), json!(id));
                }
            }
            json_bytes(&document)?
        }
        _ => return Err("此客户端的模型在客户端内选择。".into()),
    };
    Ok(vec![Change::new(path, before, after)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workbuddy_adds_a_direct_model_without_changing_other_entries() {
        let temp = tempfile::tempdir().unwrap();
        let path = Client::Workbuddy.config(temp.path());
        assert!(path.starts_with(temp.path()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            br#"{"models":[{"id":"existing"}],"availableModels":["existing"],"other":true}"#,
        )
        .unwrap();
        let provider = super::super::providers::Provider::new_direct(
            Client::Workbuddy,
            "Gateway",
            "https://fixture.example/v1",
            "fixture-key",
            "new-model",
        )
        .unwrap();
        let changes = provider.plan(temp.path()).unwrap();
        let config = json_object(changes[0].after.as_deref()).unwrap();
        assert_eq!(config["models"][0]["id"], "existing");
        assert_eq!(config["models"][1]["id"], "new-model");
        assert_eq!(config["availableModels"], json!(["existing", "new-model"]));
        assert_eq!(config["other"], true);
        assert!(!provider.view().to_string().contains("fixture-key"));
    }

    #[test]
    fn hermes_updates_a_saved_relay_and_preserves_unrelated_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = Client::Hermes.config(temp.path());
        assert!(path.starts_with(temp.path()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let user_settings = "# My settings\nterminal:\n  backend: local\n";
        std::fs::write(&path, user_settings).unwrap();
        for model in ["initial-model", "updated-model"] {
            let changes = direct_plan(
                Client::Hermes,
                temp.path(),
                DirectModel {
                    id: "direct-test",
                    name: "Test relay",
                    url: "https://fixture.example/v1",
                    model,
                    key: "fixture-key",
                },
            )
            .unwrap();
            let next = changes[0].after.as_ref().unwrap();
            assert!(next.starts_with(user_settings.as_bytes()));
            let yaml: serde_yaml::Value = serde_yaml::from_slice(next).unwrap();
            assert_eq!(yaml["custom_providers"].as_sequence().unwrap().len(), 1);
            std::fs::write(&path, next).unwrap();
            assert_eq!(
                yaml_relay_model(Client::Hermes, temp.path(), "direct-test", model).as_deref(),
                Some("initial-model")
            );
        }
    }

    #[test]
    fn relay_switch_reuses_live_model_and_keeps_existing_native_catalog() {
        for (client, config, extra, selected) in [
            (
                Client::Pi,
                r#"{"providers":{"direct-test":{"models":[{"id":"old-model","reasoning":true}]}}}"#,
                Some((
                    "settings.json",
                    r#"{"defaultProvider":"existing","defaultModel":"vendor/new-model"}"#,
                )),
                "direct-test/vendor/new-model",
            ),
            (
                Client::Opencode,
                r#"{"model":"existing/vendor/new-model","provider":{"direct-test":{"models":{"old-model":{"name":"Old"}}}}}"#,
                None,
                "direct-test/vendor/new-model",
            ),
            (
                Client::Openclaw,
                r#"{"agents":{"defaults":{"model":"existing/vendor/new-model"}}}"#,
                None,
                "direct-test/vendor/new-model",
            ),
            (
                Client::Grokbuild,
                "[models]\ndefault = 'current'\n[model.current]\nmodel = 'vendor/new-model'\n",
                None,
                "direct-test",
            ),
            (
                Client::Kimi,
                "default_model = 'current'\n[models.current]\nmodel = 'vendor/new-model'\n",
                None,
                "direct-test",
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = client.config(temp.path());
            assert!(path.starts_with(temp.path()));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, config).unwrap();
            if let Some((file, contents)) = extra {
                std::fs::write(path.with_file_name(file), contents).unwrap();
            }
            let changes = direct_plan(
                client,
                temp.path(),
                DirectModel {
                    id: "direct-test",
                    name: "Fixture",
                    url: "https://fixture.example/v1",
                    model: "stale",
                    key: "fixture-key",
                },
            )
            .unwrap();
            for change in changes {
                let output = change.after.unwrap();
                assert!(!String::from_utf8_lossy(&output).contains("stale"));
                std::fs::write(change.path, output).unwrap();
            }
            let (active, models) = catalog(client, temp.path()).unwrap();
            assert_eq!(active.as_deref(), Some(selected));
            assert_eq!(
                models
                    .iter()
                    .find(|model| model.id == selected)
                    .unwrap()
                    .model,
                "vendor/new-model"
            );
            if matches!(client, Client::Pi | Client::Opencode) {
                assert!(models
                    .iter()
                    .any(|model| model.id == "direct-test/old-model"));
            }
        }
    }

    #[test]
    fn pi_switches_existing_model_and_preserves_account_data() {
        let temp = tempfile::tempdir().unwrap();
        let path = Client::Pi.home(temp.path());
        assert!(path.starts_with(temp.path()));
        std::fs::create_dir_all(&path).unwrap();
        let models =
            json!({"providers":{"custom":{"models":[{"id":"vendor/one"},{"id":"vendor/two"}]}}});
        let settings = json!({"defaultProvider":"custom","defaultModel":"vendor/one","other":true});
        std::fs::write(path.join("models.json"), models.to_string()).unwrap();
        std::fs::write(path.join("settings.json"), settings.to_string()).unwrap();
        assert!(select(Client::Pi, temp.path(), "custom/missing").is_err());
        let changes = select(Client::Pi, temp.path(), "custom/vendor/two").unwrap();
        let updated = json_object(changes[0].after.as_deref()).unwrap();
        assert_eq!(changes[0].path, path.join("settings.json"));
        assert_eq!(updated["defaultModel"], "vendor/two");
        assert_eq!(updated["defaultProvider"], "custom");
        assert_eq!(updated["other"], true);
    }
}

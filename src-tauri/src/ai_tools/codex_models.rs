use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{json, Value};
use toml_edit::{value, DocumentMut, Item, Table};

use super::{
    providers::{
        codex_definitions, current, item_text, register_codex_route, toml, Client, Provider,
        LEGACY_CODEX_PROVIDER,
    },
    storage::{self, read, read_config, Change},
};

pub(super) const UNIFIED_PROVIDER: &str = "penguinpal-unified-history";
const MODEL_FIELDS: [&str; 2] = ["model", "model_reasoning_effort"];
const CATALOG_KEY: &str = "model_catalog_json";
const CATALOG_DIR: &str = "penguinpal-models";
const LIMIT: u64 = 2 * 1024 * 1024;
const MAX_MODELS: usize = 512;

pub(super) struct Discovery {
    pub source: String,
    ids: BTreeSet<String>,
}

fn model_ids(body: &Value) -> Result<BTreeSet<String>, String> {
    let (items, field) = if let Some(models) = body["models"].as_array() {
        (models, "slug")
    } else {
        (
            body["data"]
                .as_array()
                .ok_or("供应商返回了无法识别的模型列表。")?,
            "id",
        )
    };
    if items.len() > MAX_MODELS {
        return Err("供应商模型列表超过容量限制。".into());
    }
    let ids: BTreeSet<_> = items
        .iter()
        .filter_map(|item| item[field].as_str())
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 180
                && !id.chars().any(char::is_whitespace)
                && !id.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .collect();
    if ids.is_empty() {
        return Err("供应商没有返回可用模型。".into());
    }
    Ok(ids)
}

pub(super) async fn discover(provider: &Provider) -> Result<Discovery, String> {
    let Some((url, key)) = provider.codex_model_endpoint()? else {
        return Err("请选择使用 API Key 的 Codex 中转。".into());
    };
    let source = source_id(&url, &key);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|_| "无法准备模型目录请求。")?;
    let mut response = client
        .get(url)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| "无法连接供应商的模型目录。")?;
    if !response.status().is_success() {
        return Err(format!(
            "供应商模型目录返回 HTTP {}。当前模型目录已保留。",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "未能读取完整的模型目录。")?
    {
        if bytes.len() + chunk.len() > LIMIT as usize {
            return Err("供应商模型目录超过容量限制。".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| "供应商的模型目录不是 JSON。")?;
    Ok(Discovery {
        source,
        ids: model_ids(&body)?,
    })
}

fn source_id(url: &reqwest::Url, key: &str) -> String {
    crate::companion_sync::digest(&serde_json::to_vec(&(url.as_str(), key)).unwrap_or_default())
}

pub(super) fn source_for(provider: &Provider) -> Result<Option<String>, String> {
    Ok(provider
        .codex_model_endpoint()?
        .map(|(url, key)| source_id(&url, &key)))
}

fn managed_path(home: &Path, source: &str) -> PathBuf {
    home.join(CATALOG_DIR).join(format!("{source}.json"))
}

fn is_managed(home: &Path, path: &str) -> bool {
    let path = Path::new(path);
    path.parent() == Some(home.join(CATALOG_DIR).as_path())
        && path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        && path.extension().is_some_and(|ext| ext == "json")
}

fn metadata(path: &Path) -> impl Iterator<Item = Value> {
    read(path, LIMIT)
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|mut json| json["models"].as_array_mut().map(std::mem::take))
        .into_iter()
        .flatten()
}

fn cache_catalog(
    home: &Path,
    discovery: &Discovery,
    selected_model: &str,
) -> Result<(Option<Change>, usize), String> {
    let path = managed_path(home, &discovery.source);
    let mut known = BTreeMap::new();
    for entry in metadata(&home.join("models_cache.json")).chain(metadata(&path)) {
        if let Some(slug) = entry["slug"]
            .as_str()
            .filter(|slug| discovery.ids.contains(*slug))
        {
            known.entry(slug.to_owned()).or_insert(entry);
        }
    }
    let count = known.len();
    if count == 0 || (!selected_model.is_empty() && !known.contains_key(selected_model)) {
        return Ok((None, 0));
    }
    let bytes = serde_json::to_vec(&json!({"models": known.into_values().collect::<Vec<_>>()}))
        .map_err(|_| "无法编码 Codex 模型目录。")?;
    Ok((
        Some(Change::new(path.clone(), read_config(&path)?, bytes)),
        count,
    ))
}

fn use_catalog(doc: &mut DocumentMut, path: &Path) -> Result<(), String> {
    let path = path.to_str().ok_or("Codex 模型目录路径无效。")?;
    doc.insert(CATALOG_KEY, toml_edit::value(path));
    Ok(())
}

pub(super) fn switch_plan(user: &Path, provider: &Provider) -> Result<Vec<Change>, String> {
    let mut changes = provider.plan(user)?;
    let config = changes.last_mut().ok_or("未找到 Codex 配置修改。")?;
    let home = Client::Codex.home(user);
    let live = toml(config.before.as_deref())?;
    let mut next = toml(config.after.as_deref())?;
    let source = source_for(provider)?;
    // A provider's saved model only supplies an initial default for a fresh configuration.
    let reference = item_text(&live, CATALOG_KEY);
    if live.contains_key("model") || !reference.is_empty() {
        copy_fields(&mut next, &live, &MODEL_FIELDS);
    } else {
        copy_fields(&mut next, &live, &["model_reasoning_effort"]);
    }

    let managed = is_managed(&home, reference);
    if reference.is_empty() || managed {
        let target_catalog = source.as_deref().map(|source| managed_path(&home, source));
        if let Some(path) = target_catalog.as_deref().filter(|path| {
            let selected = item_text(&next, "model");
            metadata(path).any(|entry| {
                entry["slug"]
                    .as_str()
                    .is_some_and(|slug| selected.is_empty() || slug == selected)
            })
        }) {
            use_catalog(&mut next, path)?;
        } else if managed && target_catalog.as_deref() != Some(Path::new(reference)) {
            next.remove(CATALOG_KEY);
        }
    }
    config.after = Some(next.to_string().into_bytes());
    Ok(changes)
}

pub(super) fn unified(doc: &DocumentMut) -> bool {
    item_text(doc, "model_provider") == UNIFIED_PROVIDER
}

fn unify_selection(doc: &mut DocumentMut) -> Result<(), String> {
    let id = item_text(doc, "model_provider");
    let route = if matches!(id, "" | "openai") {
        if doc.contains_key("openai_base_url") {
            return Err("请先清除 OpenAI 默认地址覆盖，再启用会话统一。".into());
        }
        let mut official = Table::new();
        official.insert("name", value("OpenAI"));
        official.insert("requires_openai_auth", value(true));
        official.insert("supports_websockets", value(true));
        official.insert("wire_api", value("responses"));
        Item::Table(official)
    } else {
        let item = definitions(doc)
            .and_then(|entries| entries.get(id))
            .ok_or("当前 Codex 供应商缺少独立定义，请先在桌宠中切换到已保存的供应商。")?;
        let route = item.as_table_like().ok_or("当前 Codex 供应商定义无效。")?;
        if doc.contains_key("openai_base_url")
            || route.get("requires_openai_auth").and_then(Item::as_bool) == Some(true)
            || route
                .get("base_url")
                .and_then(Item::as_str)
                .is_none_or(str::is_empty)
        {
            return Err(
                "当前供应商配置无法安全共用会话标识，请先在桌宠中切换到已保存的供应商。".into(),
            );
        }
        item.clone()
    };
    codex_definitions(doc)?.insert(UNIFIED_PROVIDER, route);
    doc.insert("model_provider", value(UNIFIED_PROVIDER));
    Ok(())
}

pub(super) fn keep_unified_selection(config: &mut Change) -> Result<(), String> {
    if unified(&toml(config.before.as_deref())?) {
        let mut next = toml(config.after.as_deref())?;
        unify_selection(&mut next)?;
        config.after = Some(next.to_string().into_bytes());
    }
    Ok(())
}

pub(super) fn history_plan(user: &Path, enabled: bool) -> Result<Option<Change>, String> {
    let path = Client::Codex.config(user);
    let before = read_config(&path)?;
    let mut live = toml(before.as_deref())?;
    if unified(&live) == enabled {
        return Ok(None);
    }
    if enabled {
        unify_selection(&mut live)?;
    } else if let Some(route) = definitions(&live)
        .and_then(|entries| entries.get(UNIFIED_PROVIDER))
        .cloned()
    {
        let fields = route.as_table_like().ok_or("当前统一供应商配置无效。")?;
        if fields
            .get("base_url")
            .and_then(Item::as_str)
            .is_some_and(|url| !url.is_empty())
        {
            register_codex_route(&mut live, route)?;
        } else {
            live.remove("model_provider");
        }
    } else {
        live.remove("model_provider");
    }
    // Leave the unified definition available to the threads already bound to it.
    Ok(Some(Change::new(
        path,
        before,
        live.to_string().into_bytes(),
    )))
}

fn copy_fields(target: &mut DocumentMut, source: &DocumentMut, keys: &[&str]) {
    for key in keys {
        if let Some(value) = source.get(key) {
            if let Some(existing) = target.get_mut(key) {
                *existing = value.clone();
            } else {
                target.insert(key, value.clone());
            }
        } else {
            target.remove(key);
        }
    }
}

fn definitions(doc: &DocumentMut) -> Option<&dyn toml_edit::TableLike> {
    doc.get("model_providers")?.as_table_like()
}

pub(super) fn restore_legacy_definition(root: &Path, config: &mut Change) -> Result<(), String> {
    let mut next = toml(config.after.as_deref())?;
    if definitions(&next).is_some_and(|entries| entries.contains_key(LEGACY_CODEX_PROVIDER)) {
        return Ok(());
    }
    for id in storage::backup_ids(root)?.into_iter().take(40) {
        let Ok(backup) = storage::backup(root, &id) else {
            continue;
        };
        let route = backup
            .changes
            .iter()
            .filter(|change| change.path == config.path)
            .flat_map(|change| [&change.after, &change.before])
            .filter_map(|bytes| toml(bytes.as_deref()).ok())
            .find_map(|snapshot| {
                let item = definitions(&snapshot)?.get(LEGACY_CODEX_PROVIDER)?;
                item.as_table_like().map(|_| item.clone())
            });
        if let Some(route) = route {
            codex_definitions(&mut next)?.insert(LEGACY_CODEX_PROVIDER, route);
            config.after = Some(next.to_string().into_bytes());
            break;
        }
    }
    Ok(())
}

pub(super) fn undo_config(
    config: &Change,
    live_bytes: Option<Vec<u8>>,
    switch: bool,
) -> Result<Change, String> {
    let mut restored = toml(config.before.as_deref())?;
    let live = toml(live_bytes.as_deref())?;
    if switch {
        let saved = toml(config.after.as_deref())?;
        let mut expected = saved.clone();
        let mut comparable = live.clone();
        for key in MODEL_FIELDS.into_iter().chain([CATALOG_KEY]) {
            expected.remove(key);
            comparable.remove(key);
        }
        if expected.to_string() != comparable.to_string() {
            return Err("Codex 配置还有其它改动，无法安全回滚供应商。".into());
        }

        copy_fields(&mut restored, &live, &MODEL_FIELDS);
        let reference = item_text(&live, CATALOG_KEY);
        if reference != item_text(&saved, CATALOG_KEY)
            || (!reference.is_empty()
                && config
                    .path
                    .parent()
                    .is_none_or(|home| !is_managed(home, reference)))
        {
            copy_fields(&mut restored, &live, &[CATALOG_KEY]);
        }
    }
    if let Some(providers) = definitions(&live).filter(|table| !table.is_empty()) {
        let target = codex_definitions(&mut restored)?;
        for (id, provider) in providers.iter() {
            if id != UNIFIED_PROVIDER || !target.contains_key(id) {
                target.insert(id, provider.clone());
            }
        }
    }
    let output = restored.to_string();
    Ok(Change {
        path: config.path.clone(),
        before: live_bytes,
        after: (config.before.is_some() || !output.is_empty()).then(|| output.into_bytes()),
    })
}

pub(super) fn refresh_plan(
    user: &Path,
    before: Option<Vec<u8>>,
    discovered: &Discovery,
) -> Result<(usize, Vec<Change>), String> {
    let config = Client::Codex.config(user);
    if read_config(&config)? != before
        || source_for(&current(Client::Codex, user)?)?.as_ref() != Some(&discovered.source)
    {
        return Err("模型刷新期间 Codex 配置已变化，请重新刷新。".into());
    }
    let home = Client::Codex.home(user);
    let mut doc = toml(before.as_deref())?;
    let reference = item_text(&doc, CATALOG_KEY);
    if !reference.is_empty() && !is_managed(&home, reference) {
        return Ok((0, Vec::new()));
    }
    let (catalog, count) = cache_catalog(&home, discovered, item_text(&doc, "model"))?;
    let Some(catalog) = catalog else {
        return Ok((0, Vec::new()));
    };
    use_catalog(&mut doc, &catalog.path)?;
    Ok((
        count,
        vec![
            catalog,
            Change::new(config, before, doc.to_string().into_bytes()),
        ],
    ))
}

impl Discovery {
    pub fn count(&self) -> usize {
        self.ids.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_prefers_live_selection_and_preserves_manual_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path();
        let path = Client::Codex.config(user);
        assert!(path.starts_with(user));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "model = 'live-v2'\nmodel_reasoning_effort = 'high'\nmodel_catalog_json = 'custom.json'\n").unwrap();
        let provider = Provider::new_direct(
            Client::Codex,
            "Fixture",
            "https://fixture.example/v1",
            "fixture",
            "saved-v1",
        )
        .unwrap();
        let next = switch_plan(user, &provider).unwrap();
        let doc = toml(next.last().unwrap().after.as_deref()).unwrap();
        assert_eq!(doc["model"].as_str(), Some("live-v2"));
        assert_eq!(doc["model_reasoning_effort"].as_str(), Some("high"));
        assert_eq!(doc["model_catalog_json"].as_str(), Some("custom.json"));
        assert!(model_ids(&json!({"data":[]})).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn unified_history_preserves_auth_boundaries_and_is_reversible() {
        let fixture = tempfile::tempdir().unwrap();
        let user = fixture.path();
        let path = Client::Codex.config(user);
        assert!(path.starts_with(user));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "model = 'live-model'\n").unwrap();
        let enable = history_plan(user, true).unwrap().unwrap();
        storage::commit(user, "开启统一 Codex 会话历史", vec![enable]).unwrap();
        assert_eq!(
            current(Client::Codex, user).unwrap().view()["official"],
            true
        );
        assert!(history_plan(user, true).unwrap().is_none());
        let relay = Provider::new_direct(
            Client::Codex,
            "Fixture",
            "https://fixture.example/v1",
            "fixture-key",
            "old-model",
        )
        .unwrap();
        let mut forward = switch_plan(user, &relay).unwrap();
        keep_unified_selection(&mut forward[0]).unwrap();
        let relay_config = toml(forward[0].after.as_deref()).unwrap();
        assert!(unified(&relay_config));
        assert_eq!(item_text(&relay_config, "model"), "live-model");
        let alias = &relay_config["model_providers"][UNIFIED_PROVIDER];
        assert_eq!(
            alias["base_url"].as_str(),
            Some("https://fixture.example/v1")
        );
        assert!(alias.get("requires_openai_auth").is_none());
        let switch = storage::commit(user, "Codex · 切换到中转", forward)
            .unwrap()
            .unwrap();
        storage::rollback(user, &switch, user).unwrap();
        let rolled_back = toml(read_config(&path).unwrap().as_deref()).unwrap();
        let alias = &rolled_back["model_providers"][UNIFIED_PROVIDER];
        assert_eq!(alias["requires_openai_auth"].as_bool(), Some(true));
        assert!(alias.get("base_url").is_none());
        assert!(alias.get("experimental_bearer_token").is_none());
        assert!(unified(&rolled_back));

        let mut forward = switch_plan(user, &relay).unwrap();
        keep_unified_selection(&mut forward[0]).unwrap();
        storage::commit(user, "Codex · 切换到中转", forward).unwrap();
        let disable = history_plan(user, false).unwrap().unwrap();
        let normal = toml(disable.after.as_deref()).unwrap();
        assert!(!unified(&normal));
        let active = item_text(&normal, "model_provider");
        let definitions = definitions(&normal).unwrap();
        assert_eq!(
            definitions.get(active).unwrap().to_string(),
            definitions.get(UNIFIED_PROVIDER).unwrap().to_string()
        );
    }

    #[test]
    fn unification_rejects_official_credentials_on_a_relay_route() {
        let mut doc = toml(Some(b"model_provider = 'external'\n[model_providers.external]\nbase_url = 'https://fixture.example/v1'\nrequires_openai_auth = true\n")).unwrap();
        let original = doc.to_string();
        assert!(unify_selection(&mut doc).is_err());
        assert_eq!(doc.to_string(), original);
    }

    #[cfg(windows)]
    #[test]
    fn restores_old_definition_without_changing_current_provider_or_model() {
        let fixture = tempfile::tempdir().unwrap();
        let user = fixture.path();
        let path = Client::Codex.config(user);
        assert!(path.starts_with(user));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let before = b"model_provider = 'penguinpal-direct'\n[model_providers.penguinpal-direct]\nname = 'Old relay'\nbase_url = 'https://fixture.example/v1'\nwire_api = 'responses'\n";
        let after = b"model = 'new-model'\nmodel_provider = 'openai'\n";
        std::fs::write(&path, before).unwrap();
        storage::commit(
            user,
            "Codex · 切换到官方",
            vec![Change::new(
                path.clone(),
                Some(before.to_vec()),
                after.to_vec(),
            )],
        )
        .unwrap();
        let mut change = Change::new(path.clone(), Some(after.to_vec()), after.to_vec());
        restore_legacy_definition(user, &mut change).unwrap();
        let fixed = toml(change.after.as_deref()).unwrap();
        assert_eq!(fixed["model_provider"].as_str(), Some("openai"));
        assert_eq!(fixed["model"].as_str(), Some("new-model"));
        assert_eq!(
            fixed["model_providers"][LEGACY_CODEX_PROVIDER]["name"].as_str(),
            Some("Old relay")
        );
        let restored = change.after.clone();
        restore_legacy_definition(user, &mut change).unwrap();
        assert_eq!(change.after, restored);
    }

    #[cfg(windows)]
    #[test]
    fn switching_and_undo_keep_definitions_for_both_relays() {
        let fixture = tempfile::tempdir().unwrap();
        let user = fixture.path();
        let config = Client::Codex.config(user);
        assert!(config.starts_with(user));
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let relay = |label, endpoint| {
            Provider::new_direct(Client::Codex, label, endpoint, "fixture-key", "saved-model")
                .unwrap()
        };
        let first = relay("First relay", "https://first.fixture.example/v1");
        let second = relay("Second relay", "https://second.fixture.example/v1");
        std::fs::write(&config, "model = 'live-model'\n").unwrap();
        let mut ids = Vec::new();
        for provider in [&first, &second] {
            let changes = switch_plan(user, provider).unwrap();
            let doc = toml(changes[0].after.as_deref()).unwrap();
            ids.push(item_text(&doc, "model_provider").to_owned());
            let id = storage::commit(user, "Codex · 切换到中转", changes)
                .unwrap()
                .unwrap();
            if ids.len() == 2 {
                storage::rollback(user, &id, user).unwrap();
            }
        }
        assert_ne!(ids[0], ids[1]);
        let restored = toml(read_config(&config).unwrap().as_deref()).unwrap();
        assert_eq!(item_text(&restored, "model_provider"), ids[0]);
        assert_eq!(item_text(&restored, "model"), "live-model");
        let official = switch_plan(user, &Provider::official(Client::Codex)).unwrap();
        let doc = toml(official[0].after.as_deref()).unwrap();
        assert!(doc.get("model_provider").is_none());
        let defs = definitions(&doc).unwrap();
        assert_eq!(
            defs.get(&ids[0]).unwrap()["base_url"].as_str(),
            Some("https://first.fixture.example/v1")
        );
        assert_eq!(
            defs.get(&ids[1]).unwrap()["base_url"].as_str(),
            Some("https://second.fixture.example/v1")
        );
        storage::commit(user, "Codex · 切换到官方", official).unwrap();
        let again = switch_plan(user, &first).unwrap();
        let doc = toml(again[0].after.as_deref()).unwrap();
        assert_eq!(item_text(&doc, "model_provider"), ids[0]);
        assert_eq!(definitions(&doc).unwrap().len(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn rolling_back_an_old_provider_switch_keeps_newer_model_settings() {
        use super::super::storage;
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path();
        let path = Client::Codex.config(user);
        assert!(path.starts_with(user));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let before = b"model = 'model-at-switch'\nmodel_provider = 'openai'\n";
        let after = b"model = 'stale-saved-model'\nmodel_provider = 'relay'\n";
        std::fs::write(&path, before).unwrap();
        let id = storage::commit(
            user,
            "Codex · 切换到 Fixture",
            vec![Change::new(
                path.clone(),
                Some(before.to_vec()),
                after.to_vec(),
            )],
        )
        .unwrap()
        .unwrap();

        let live = "model_provider = 'relay'\nmodel = 'newer-model'\nmodel_catalog_json = 'local.json'\nmodel_reasoning_effort = 'high'\n";
        std::fs::write(&path, format!("{live}other_setting = true\n")).unwrap();
        assert!(storage::rollback(user, &id, user).is_err());
        std::fs::write(&path, live).unwrap();
        storage::rollback(user, &id, user).unwrap();
        let doc = toml(read_config(&path).unwrap().as_deref()).unwrap();
        assert_eq!(doc["model_provider"].as_str(), Some("openai"));
        assert_eq!(doc["model"].as_str(), Some("newer-model"));
        assert_eq!(doc[CATALOG_KEY].as_str(), Some("local.json"));
        assert_eq!(doc["model_reasoning_effort"].as_str(), Some("high"));
    }

    #[test]
    fn refresh_preserves_manual_catalog_and_updates_managed_catalog_from_current_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path();
        let provider = Provider::new_direct(
            Client::Codex,
            "Fixture",
            "https://fixture.example/v1",
            "fixture",
            "model-a",
        )
        .unwrap();
        let path = Client::Codex.config(user);
        assert!(path.starts_with(user));
        let base_config = provider.plan(user).unwrap().pop().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let discovery = Discovery {
            source: source_for(&provider).unwrap().unwrap(),
            ids: model_ids(&json!({"data":[{"id":"model-a"},{"id":"model-b"}]})).unwrap(),
        };
        let mut doc = toml(base_config.after.as_deref()).unwrap();
        doc[CATALOG_KEY] = toml_edit::value("manual.json");
        std::fs::write(&path, doc.to_string()).unwrap();
        let before = read_config(&path).unwrap();
        assert_eq!(refresh_plan(user, before, &discovery).unwrap().1.len(), 0);

        doc.remove(CATALOG_KEY);
        std::fs::write(&path, doc.to_string()).unwrap();
        std::fs::write(
            path.with_file_name("models_cache.json"),
            json!({"models":[{"slug":"model-a"},{"slug":"model-b"},{"slug":"other"}]}).to_string(),
        )
        .unwrap();
        let before = read_config(&path).unwrap();
        let (count, changes) = refresh_plan(user, before, &discovery).unwrap();
        assert_eq!(count, 2);
        assert_eq!(changes.len(), 2);
        let merged: Value = serde_json::from_slice(changes[0].after.as_deref().unwrap()).unwrap();
        assert_eq!(model_ids(&merged).unwrap(), discovery.ids);
        let doc = toml(changes[1].after.as_deref()).unwrap();
        assert_eq!(doc["model"].as_str(), Some("model-a"));
        assert!(is_managed(
            path.parent().unwrap(),
            item_text(&doc, CATALOG_KEY)
        ));
    }

    #[test]
    fn a_different_relay_does_not_inherit_the_previous_managed_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path();
        let config = Client::Codex.config(user);
        assert!(config.starts_with(user));
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let provider = Provider::new_direct(
            Client::Codex,
            "Fixture",
            "https://fixture.example/v1",
            "fixture",
            "one",
        )
        .unwrap();
        let local = managed_path(&Client::Codex.home(user), &"a".repeat(64));
        let mut doc = DocumentMut::new();
        use_catalog(&mut doc, &local).unwrap();
        std::fs::write(&config, doc.to_string()).unwrap();
        let next = switch_plan(user, &provider).unwrap();
        assert!(toml(next.last().unwrap().after.as_deref())
            .unwrap()
            .get("model_catalog_json")
            .is_none());
    }
}

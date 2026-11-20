use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{json, Value};
use toml_edit::DocumentMut;

use super::{
    providers::{current, item_text, toml, Client, Provider},
    storage::{read, read_config, Change},
};

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

pub(super) fn undo_switch(config: &Change, live_bytes: Option<Vec<u8>>) -> Result<Change, String> {
    let before = toml(config.before.as_deref())?;
    let saved = toml(config.after.as_deref())?;
    let live = toml(live_bytes.as_deref())?;
    let mut expected = saved.clone();
    let mut comparable = live.clone();
    for key in MODEL_FIELDS.into_iter().chain([CATALOG_KEY]) {
        expected.remove(key);
        comparable.remove(key);
    }
    if expected.to_string() != comparable.to_string() {
        return Err("Codex 配置还有其它改动，无法安全回滚供应商。".into());
    }

    let mut restored = before;
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

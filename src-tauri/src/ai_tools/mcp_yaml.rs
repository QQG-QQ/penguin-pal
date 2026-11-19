use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use serde_yaml::Value as Yaml;

use super::{
    providers::Client,
    storage::{read_config, Change},
};

const DSH_PLUGIN: &str = "@deepseek-ai/dsh-mcp-client";

pub(super) fn dsh_direct_plan(
    user: &Path,
    direct: super::native_models::DirectModel<'_>,
) -> Result<Vec<Change>, String> {
    let path = dsh_home_patch(user);
    let before = read_config(&path)?;
    let text = source(before.as_deref())?;
    let existing = dsh_patches(text)?;
    let start = "# PenguinPal direct provider";
    let end = "# End PenguinPal direct provider";
    let markers: Vec<_> = text
        .split_inclusive('\n')
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some((start, *offset, line.trim_end_matches(['\r', '\n'])))
        })
        .filter(|(_, _, line)| *line == start || *line == end)
        .collect();
    let prefix = match markers.as_slice() {
        [] => text.to_owned(),
        [(start_at, body_at, first), (end_at, next_at, last)]
            if *first == start && *last == end =>
        {
            let body = &text[*body_at..*end_at];
            ensure_plain(body)?;
            let stored = dsh_patches(body)?;
            if stored.len() != 2
                || stored[0]["id"].as_str() != Some("llm-pi-ai")
                || stored[1]["id"].as_str() != Some("agent-default-model")
            {
                return Err("DeepSeek Harness 中转区块包含额外配置，请先在客户端核对。".into());
            }
            format!("{}{}", &text[..*start_at], &text[*next_at..])
        }
        _ => return Err("DeepSeek Harness 中转区块边界不完整。".into()),
    };
    let local = dsh_patches(&prefix)?;
    if local.iter().any(|patch| {
        patch["id"].as_str() == Some("llm-pi-ai")
            || patch["id"].as_str() == Some("agent-default-model")
    }) {
        return Err("DeepSeek Harness 全局配置已有同类覆盖项，请先在客户端核对配置。".into());
    }
    let reference = format!(
        "PENGUINPAL_DSH_{}",
        direct.id.replace('-', "_").to_ascii_uppercase()
    );
    let credentials = Client::Dsh.home(user).join(".credentials.yaml");
    let previous = read_config(&credentials)?;
    let mut stored = parse(source(previous.as_deref())?)?;
    if stored.is_null() {
        stored = Yaml::Mapping(Default::default());
    }
    let mapping = stored
        .as_mapping_mut()
        .ok_or("DeepSeek 登录凭据格式无效。")?;
    let refs = mapping
        .entry(Yaml::from("refs"))
        .or_insert_with(|| Yaml::Mapping(Default::default()));
    refs.as_mapping_mut()
        .ok_or("DeepSeek 直连密钥引用格式无效。")?
        .insert(Yaml::from(reference.as_str()), Yaml::from(direct.key));
    let (_, active_models) = super::native_models::catalog(Client::Dsh, user)?;
    let model = active_models
        .first()
        .map_or(direct.model, |entry| entry.model.as_str());
    let live_provider = existing.iter().find_map(|plugin| {
        (plugin["id"].as_str() == Some("llm-pi-ai"))
            .then(|| plugin["config"]["providers"].get(direct.id))
            .flatten()
    });
    let mut provider = live_provider
        .map(as_json)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    super::native_models::merge_provider(
        &mut provider,
        json!({
            "displayName": direct.name, "api": "openai-completions", "baseURL": direct.url,
            "apiKeyEnv": reference,
            "models": [{"id":model,"name":model,"contextWindow":128000,"maxTokens":8192,"input":["text"]}]
        }),
    )?;
    let patch = json!([
        {"id":"llm-pi-ai","config":{"providers":{(direct.id):provider}}},
        {"id":"agent-default-model","config":{"provider":direct.id,"model":model}}
    ]);
    let yaml = serde_yaml::to_string(&patch).map_err(|_| "无法编码 DeepSeek Harness 配置。")?;
    let prefix = if local.is_empty() {
        yaml_comments(&prefix)
    } else {
        newline(&prefix)
    };
    let next = format!("{prefix}{start}\n{yaml}{end}\n");
    dsh_patches(&next)?;
    let auth = serde_yaml::to_string(&stored).map_err(|_| "无法保存 DeepSeek 直连密钥。")?;
    Ok(vec![
        Change::new(credentials, previous, auth.into_bytes()),
        Change::new(path, before, next.into_bytes()),
    ])
}

fn source(bytes: Option<&[u8]>) -> Result<&str, String> {
    std::str::from_utf8(bytes.unwrap_or_default())
        .map(|text| text.trim_start_matches('\u{feff}'))
        .map_err(|_| "MCP 配置不是 UTF-8。".into())
}

fn parse(text: &str) -> Result<Yaml, String> {
    serde_yaml::from_str(text).map_err(|_| "现有 MCP YAML 配置无法读取。".into())
}

fn as_json(yaml: &Yaml) -> Result<Value, String> {
    serde_json::to_value(yaml).map_err(|_| "MCP 配置含不支持的 YAML 值。".into())
}

pub(super) fn hermes_entries(bytes: Option<&[u8]>) -> Result<Option<Value>, String> {
    let doc = parse(source(bytes)?)?;
    Ok(doc.get("mcp_servers").map(as_json).transpose()?)
}

pub(super) fn put_hermes(
    bytes: Option<&[u8]>,
    name: Option<&str>,
    entries: Map<String, Value>,
) -> Result<Vec<u8>, String> {
    let text = source(bytes)?;
    let doc = parse(text)?;
    if !doc.is_null() && !doc.is_mapping() {
        return Err("Hermes 配置必须是 YAML 对象。".into());
    }
    let mut updated = if name.is_some() {
        hermes_entries(bytes)?
            .unwrap_or_else(|| json!({}))
            .as_object()
            .cloned()
            .ok_or("Hermes 的 mcp_servers 必须是对象。")?
    } else {
        Map::new()
    };
    if let Some(name) = name {
        updated.remove(name);
    }
    updated.extend(entries);
    let mut expected = if doc.is_null() {
        Yaml::Mapping(Default::default())
    } else {
        doc.clone()
    };
    expected["mcp_servers"] = serde_yaml::to_value(updated).map_err(|_| "无法编码 Hermes MCP。")?;
    replace_yaml_fields(text, &doc, &expected, &["mcp_servers"])
}

pub(super) fn replace_yaml_fields(
    text: &str,
    original: &Yaml,
    expected: &Yaml,
    fields: &[&str],
) -> Result<Vec<u8>, String> {
    let mut next = if original.is_null() || original.as_mapping().is_some_and(|map| map.is_empty())
    {
        yaml_comments(text)
    } else {
        newline(text)
    };
    for field in fields {
        let key = format!("{field}:");
        let mut entry = serde_yaml::Mapping::new();
        entry.insert(Yaml::from(*field), expected[*field].clone());
        let replacement = serde_yaml::to_string(&entry).map_err(|_| "无法编码 YAML 配置。")?;
        let start = next.match_indices(&key).find_map(|(index, _)| {
            (index == 0 || next.as_bytes()[index - 1] == b'\n').then_some(index)
        });
        if let Some(start) = start {
            let end = next[start..]
                .split_inclusive('\n')
                .scan(start, |offset, line| {
                    let start = *offset;
                    *offset += line.len();
                    Some((start, line))
                })
                .skip(1)
                .find(|(_, line)| {
                    !line.trim().is_empty()
                        && !line.starts_with(char::is_whitespace)
                        && !line.starts_with("- ")
                })
                .map_or(next.len(), |(end, _)| end);
            ensure_plain(&next[start..end])?;
            next.replace_range(start..end, &replacement);
        } else {
            next.push_str(&replacement);
        }
    }
    if parse(&next)? != *expected {
        return Err("现有 YAML 布局无法安全更新，请在原客户端编辑。".into());
    }
    Ok(next.into_bytes())
}

fn newline(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.into()
    } else {
        format!("{text}\n")
    }
}

fn yaml_comments(text: &str) -> String {
    newline(
        &text
            .lines()
            .filter(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
            .map(|line| format!("{line}\n"))
            .collect::<String>(),
    )
}

fn ensure_plain(text: &str) -> Result<(), String> {
    if text
        .split_whitespace()
        .any(|word| word.starts_with(['!', '&', '*']))
    {
        return Err("此配置区块使用 YAML 标签、锚点或引用，请在原客户端编辑。".into());
    }
    Ok(())
}

fn dsh_paths(user: &Path) -> Vec<PathBuf> {
    let home = Client::Dsh.home(user);
    let mut paths = Vec::new();
    for name in ["web", "desktop", "headless", "acp"] {
        let path = home.join("profiles").join(name).join("cordis.patch.yml");
        if path.is_file() || matches!(name, "web" | "desktop") {
            paths.push(path);
        }
    }
    paths
}

fn dsh_home_patch(user: &Path) -> PathBuf {
    Client::Dsh.home(user).join("cordis.patch.yml")
}

fn dsh_patches(text: &str) -> Result<Vec<Yaml>, String> {
    match parse(text)? {
        Yaml::Null => Ok(Vec::new()),
        Yaml::Sequence(list) => Ok(list),
        _ => Err("DeepSeek Harness 配置必须是 YAML 列表。".into()),
    }
}

fn plugins(patch: &Yaml) -> impl Iterator<Item = (Option<usize>, &Yaml)> {
    std::iter::once((None, patch)).chain(
        patch["insert"]
            .as_sequence()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, item)| (Some(i), item)),
    )
}

pub(super) fn dsh_entries(user: &Path) -> Result<Map<String, Value>, String> {
    let mut merged = Map::new();
    for path in std::iter::once(dsh_home_patch(user)).chain(dsh_paths(user)) {
        let bytes = read_config(&path)?;
        for (name, config) in dsh_file_entries(source(bytes.as_deref())?)? {
            merged.entry(name).or_insert(config);
        }
    }
    Ok(merged)
}

fn dsh_file_entries(text: &str) -> Result<Map<String, Value>, String> {
    let mut entries = Map::new();
    for patch in dsh_patches(text)? {
        for (_, plugin) in
            plugins(&patch).filter(|(_, entry)| entry["name"].as_str() == Some(DSH_PLUGIN))
        {
            if let Some(name) = plugin["config"]["serverName"].as_str() {
                let mut config = as_json(&plugin["config"])?;
                config["disabled"] = json!(plugin["disabled"].as_bool().unwrap_or(false));
                if entries.insert(name.to_owned(), config).is_some() {
                    return Err("DeepSeek Harness 有多个同名 MCP。".into());
                }
            }
        }
    }
    Ok(entries)
}

fn edit_dsh(text: &str, name: &str, definition: Option<&Value>) -> Result<String, String> {
    let mut patches = dsh_patches(text)?;
    let mut found = None;
    let default_id = format!("penguinpal-mcp-{name}");
    let mut id_taken = false;
    for (i, patch) in patches.iter().enumerate() {
        for (j, plugin) in plugins(patch) {
            if plugin["name"].as_str() == Some(DSH_PLUGIN)
                && plugin["config"]["serverName"].as_str() == Some(name)
            {
                if found.replace((i, j)).is_some() {
                    return Err("DeepSeek Harness 有多个同名 MCP。".into());
                }
            } else if plugin["id"].as_str() == Some(&default_id) {
                id_taken = true;
            }
        }
    }
    if found.is_none() && definition.is_none() {
        return Ok(text.into());
    }
    if found.is_none() && id_taken {
        return Err("DeepSeek Harness 已有同名插件编号。".into());
    }
    let mut starts: Vec<_> = text
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .chain(std::iter::once(0))
        .filter(|i| {
            text[*i..].starts_with("- ")
                || text[*i..]
                    .lines()
                    .next()
                    .is_some_and(|line| line.trim_end() == "-")
        })
        .collect();
    starts.sort_unstable();
    if starts.len() != patches.len() {
        return Err("DeepSeek Harness 的配置须使用块式 YAML 列表。".into());
    }
    let (prefix, suffix, replacement) = if let Some((i, child)) = found {
        let end = starts.get(i + 1).copied().unwrap_or(text.len());
        let section = &text[starts[i]..end];
        ensure_plain(section)?;
        let patch = &mut patches[i];
        let mut plugin = child
            .map(|j| patch["insert"][j].clone())
            .unwrap_or_else(|| patch.clone());
        let updated = if let Some(definition) = definition {
            let mut data = definition.clone();
            data["serverName"] = json!(name);
            data.as_object_mut().unwrap().remove("disabled");
            plugin["config"] = serde_yaml::to_value(data).map_err(|_| "无法编码 MCP 设置。")?;
            Some(plugin)
        } else {
            None
        };
        let keep = if let Some(j) = child {
            let inserted = patch["insert"].as_sequence_mut().unwrap();
            if let Some(updated) = updated {
                inserted[j] = updated;
            } else {
                inserted.remove(j);
            }
            (!inserted.is_empty() || patch.as_mapping().is_some_and(|map| map.len() > 1))
                .then(|| patch.clone())
        } else {
            updated
        };
        if let Some(keep) = &keep {
            patches[i] = keep.clone();
        } else {
            patches.remove(i);
        }
        (text[..starts[i]].to_owned(), &text[end..], keep)
    } else {
        let mut data = definition.cloned().unwrap();
        data["serverName"] = json!(name);
        data.as_object_mut().unwrap().remove("disabled");
        let plugin = json!({"insert":[{"id":default_id,"name":DSH_PLUGIN,"config":data}]});
        let patch = serde_yaml::to_value(plugin).map_err(|_| "无法编码 MCP 设置。")?;
        let prefix = if patches.is_empty() {
            yaml_comments(text)
        } else {
            newline(text)
        };
        patches.push(patch.clone());
        (prefix, "", Some(patch))
    };
    let mut encoded = replacement
        .map(|item| serde_yaml::to_string(&[item]))
        .transpose()
        .map_err(|_| "无法编码 DeepSeek Harness 配置。")?
        .unwrap_or_default();
    if patches.is_empty() {
        encoded = "[]\n".into();
    }
    let next = format!("{prefix}{encoded}{suffix}");
    if dsh_patches(&next)? != patches {
        return Err("此变更会影响其他 DeepSeek Harness 插件。".into());
    }
    Ok(next)
}

pub(super) fn plan_dsh(
    user: &Path,
    name: &str,
    previous: Option<&Value>,
    definition: Option<&Value>,
) -> Result<Vec<Change>, String> {
    if name.len() > 32 {
        return Err("DeepSeek Harness 的 MCP 名称长度上限是 32。".into());
    }
    let mut changes = Vec::new();
    let global = dsh_home_patch(user);
    let global_before = read_config(&global)?;
    let in_global = dsh_file_entries(source(global_before.as_deref())?)?.contains_key(name);
    let mut paths = dsh_paths(user);
    if in_global {
        paths.insert(0, global.clone());
    }
    for path in paths {
        let before = read_config(&path)?;
        let original = source(before.as_deref())?;
        let existing = dsh_file_entries(original)?;
        let found = existing.get(name);
        if found.is_some() && found != previous {
            return Err("DeepSeek Harness 的同名 MCP 在不同配置中存在差异，请先核对。".into());
        }
        if in_global && found.is_none() {
            continue;
        }
        let edited = edit_dsh(original, name, definition)?;
        if edited != original {
            changes.push(Change::new(path, before, edited.into_bytes()));
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsh_relay_switch_preserves_live_default_and_adds_it_to_the_target() {
        let temp = tempfile::tempdir().unwrap();
        let home = Client::Dsh.home(temp.path());
        assert!(home.starts_with(temp.path()));
        let desktop = home.join("profiles/desktop/cordis.patch.yml");
        std::fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        std::fs::write(&desktop, "- id: agent-default-model\n  config:\n    provider: deepseek-account\n    model: live-model\n").unwrap();
        let changes = dsh_direct_plan(
            temp.path(),
            super::super::native_models::DirectModel {
                id: "direct-test",
                name: "Fixture",
                url: "https://fixture.example/v1",
                model: "stale",
                key: "fixture-key",
            },
        )
        .unwrap();
        let patches = dsh_patches(source(changes[1].after.as_deref()).unwrap()).unwrap();
        assert_eq!(patches[1]["config"]["model"].as_str(), Some("live-model"));
        assert_eq!(
            patches[0]["config"]["providers"]["direct-test"]["models"][0]["id"].as_str(),
            Some("live-model")
        );
        assert!(changes.iter().all(|change| change.path != desktop));
    }

    #[test]
    fn dsh_edits_only_mcp_patch_and_keeps_dynamic_yaml() {
        let existing = "- id: user-ext\n  config:\n    value: !!js '1 + 2'\n";
        let spec = json!({"transport":"stdio","command":"fixture"});
        let added = edit_dsh(existing, "fixtures", Some(&spec)).unwrap();
        assert!(added.starts_with(existing));
        assert_eq!(
            dsh_file_entries(&added).unwrap()["fixtures"]["command"],
            "fixture"
        );
        let http = json!({"transport":"streamable-http","url":"https://fixture.example/mcp"});
        let updated = edit_dsh(&added, "fixtures", Some(&http)).unwrap();
        assert!(updated.starts_with(existing));
        assert_eq!(
            dsh_file_entries(&updated).unwrap()["fixtures"]["url"],
            http["url"]
        );
        assert_eq!(edit_dsh(&updated, "fixtures", None).unwrap(), existing);
        let empty = edit_dsh("# Profile\n[]\n", "fixtures", Some(&spec)).unwrap();
        assert_eq!(
            edit_dsh(&empty, "fixtures", None).unwrap(),
            "# Profile\n[]\n"
        );
    }

    #[test]
    fn hermes_keeps_unrelated_yaml_and_comments() {
        let input = b"# User root\nmodel: chosen\nmcp_servers: {}\nterminal:\n  backend: local\n";
        let entries = [("test".into(), json!({"command":"fixture"}))]
            .into_iter()
            .collect();
        let next = put_hermes(Some(input), Some("test"), entries).unwrap();
        let text = std::str::from_utf8(&next).unwrap();
        assert!(text.starts_with("# User root\nmodel: chosen\n"));
        assert!(text.ends_with("terminal:\n  backend: local\n"));
        assert_eq!(
            hermes_entries(Some(&next)).unwrap().unwrap()["test"]["command"],
            "fixture"
        );
    }
}

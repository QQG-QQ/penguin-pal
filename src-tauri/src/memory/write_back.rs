//! Memory Write-back - 任务完成后的记忆写入
//!
//! 负责在任务完成后将经验写入各类 memory。

use super::store::MemoryStore;
use super::types::{
    now_millis, EpisodicEntry, FrequentPath, KeyEntity, ProceduralEntry, RuntimeContextDigest,
    StableWindowFeatures, WriteBackRequest,
};

/// 写回任务结果到 memory
pub fn write_back_task_result(store: &MemoryStore, request: WriteBackRequest) -> Result<(), String> {
    let timestamp = now_millis();

    // 1. 写入 Episodic Memory
    write_episodic_entry(store, &request, timestamp)?;

    // 2. 如果成功，更新 Procedural Memory
    if request.final_status == "completed" {
        update_procedural_on_success(store, &request, timestamp)?;
    } else {
        // 失败时降低 procedural memory 的置信度
        update_procedural_on_failure(store, &request)?;
    }

    // 3. 更新 Profile Memory (常用路径、应用等)
    update_profile_from_task(store, &request)?;

    Ok(())
}

/// 写入 Episodic Entry
fn write_episodic_entry(
    store: &MemoryStore,
    request: &WriteBackRequest,
    timestamp: u64,
) -> Result<(), String> {
    // 生成标签
    let mut tags = Vec::new();
    tags.push(request.intent.clone());
    if request.final_status == "completed" {
        tags.push("success".to_string());
    } else {
        tags.push("failure".to_string());
    }
    if let Some(ref window_title) = request.runtime_context_digest.active_window_title {
        // 提取应用名作为标签
        if let Some(app_name) = extract_app_name(window_title) {
            tags.push(format!("app:{}", app_name));
        }
    }
    if request.used_retry {
        tags.push("used_retry".to_string());
    }
    if request.used_probe {
        tags.push("used_probe".to_string());
    }

    let entry = EpisodicEntry {
        id: format!("ep-{}", timestamp),
        timestamp,
        goal: request.goal.clone(),
        intent: request.intent.clone(),
        final_status: request.final_status.clone(),
        failure_reason_code: request.failure_reason_code.clone(),
        failure_stage: request.failure_stage.clone(),
        runtime_context_digest: request.runtime_context_digest.clone(),
        key_entities: request.key_entities.clone(),
        used_tools: request.used_tools.clone(),
        used_retry: request.used_retry,
        used_probe: request.used_probe,
        steps_taken: request.steps_taken,
        tags,
    };

    store.add_episodic_entry(entry)
}

/// 成功时更新 Procedural Memory
fn update_procedural_on_success(
    store: &MemoryStore,
    request: &WriteBackRequest,
    timestamp: u64,
) -> Result<(), String> {
    // 只有使用了工具的任务才写入 procedural memory
    if request.used_tools.is_empty() {
        return Ok(());
    }

    // 从 runtime context 提取稳定特征
    let stable_window_features = request
        .runtime_context_digest
        .active_window_title
        .as_ref()
        .map(|title| StableWindowFeatures {
            title_pattern: title.clone(),
            class_name: request.runtime_context_digest.active_window_class.clone(),
            process_name: None,
        });

    // 尝试加载现有的 procedural entry
    let procedural = store.load_procedural()?;
    let existing = procedural.procedures.iter().find(|p| {
        p.target_pattern == request.goal || {
            if let Some(ref features) = stable_window_features {
                if let Some(ref p_features) = p.stable_window_features {
                    p_features.title_pattern == features.title_pattern
                } else {
                    false
                }
            } else {
                false
            }
        }
    });

    let entry = if let Some(existing) = existing {
        // 更新现有条目
        let mut entry = existing.clone();
        entry.success_count += 1;
        entry.last_verified_at = timestamp;
        entry.updated_at = timestamp;
        // 提高置信度
        entry.confidence = (entry.confidence + 0.1).min(1.0);
        // 如果当前使用的工具序列更短，更新
        if request.used_tools.len() < entry.preferred_tool_sequence.len()
            || entry.preferred_tool_sequence.is_empty()
        {
            entry.preferred_tool_sequence = request.used_tools.clone();
        }
        entry
    } else {
        // 创建新条目
        ProceduralEntry {
            id: format!("proc-{}", timestamp),
            created_at: timestamp,
            updated_at: timestamp,
            target_kind: infer_target_kind(&request.key_entities),
            stable_window_features,
            stable_element_features: None, // 从 key_entities 可以提取，暂不实现
            preferred_tool_sequence: request.used_tools.clone(),
            success_count: 1,
            failure_count: 0,
            confidence: 0.5, // 初始置信度
            last_verified_at: timestamp,
            target_pattern: request.goal.clone(),
        }
    };

    store.upsert_procedural_entry(entry)
}

/// 失败时更新 Procedural Memory
fn update_procedural_on_failure(store: &MemoryStore, request: &WriteBackRequest) -> Result<(), String> {
    let procedural = store.load_procedural()?;

    // 找到匹配的 procedural entry
    let matching = procedural.procedures.iter().find(|p| {
        p.target_pattern == request.goal
            || request
                .runtime_context_digest
                .active_window_title
                .as_ref()
                .map(|title| {
                    p.stable_window_features
                        .as_ref()
                        .map(|f| f.title_pattern == *title)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
    });

    if let Some(existing) = matching {
        let mut entry = existing.clone();
        entry.failure_count += 1;
        entry.updated_at = now_millis();
        // 降低置信度
        entry.confidence = (entry.confidence - 0.1).max(0.0);
        store.upsert_procedural_entry(entry)?;
    }

    Ok(())
}

/// 从任务更新 Profile Memory
fn update_profile_from_task(store: &MemoryStore, request: &WriteBackRequest) -> Result<(), String> {
    store.update_profile(|profile| {
        // 更新常用应用
        if let Some(ref window_title) = request.runtime_context_digest.active_window_title {
            if let Some(app_name) = extract_app_name(window_title) {
                let count = profile.preferred_apps.entry(app_name).or_insert(0);
                *count += 1;
            }
        }

        // 更新常用路径 (从 key_entities 中提取文件路径)
        for entity in &request.key_entities {
            if entity.entity_type == "file" {
                let existing = profile
                    .frequently_used_paths
                    .iter_mut()
                    .find(|p| p.path == entity.id);
                if let Some(existing) = existing {
                    existing.usage_count += 1;
                    existing.last_used_at = now_millis();
                } else {
                    profile.frequently_used_paths.push(FrequentPath {
                        path: entity.id.clone(),
                        usage_count: 1,
                        last_used_at: now_millis(),
                    });
                }
            }
        }

        // 保持 frequently_used_paths 在合理范围内
        if profile.frequently_used_paths.len() > 50 {
            profile
                .frequently_used_paths
                .sort_by(|a, b| b.usage_count.cmp(&a.usage_count));
            profile.frequently_used_paths.truncate(50);
        }
    })
}

/// 写入确认被拒绝的失败经验
pub fn write_confirmation_rejected(
    store: &MemoryStore,
    goal: &str,
    tool: &str,
    window_title: Option<&str>,
) -> Result<(), String> {
    let timestamp = now_millis();

    let entry = EpisodicEntry {
        id: format!("ep-{}", timestamp),
        timestamp,
        goal: goal.to_string(),
        intent: "desktop_action".to_string(),
        final_status: "cancelled".to_string(),
        failure_reason_code: Some("confirmation_rejected".to_string()),
        failure_stage: Some("confirmation".to_string()),
        runtime_context_digest: RuntimeContextDigest {
            active_window_title: window_title.map(String::from),
            active_window_class: None,
            had_vision_context: false,
            had_uia_context: false,
            clipboard_preview: None,
        },
        key_entities: vec![],
        used_tools: vec![tool.to_string()],
        used_retry: false,
        used_probe: false,
        steps_taken: 1,
        tags: vec![
            "failure".to_string(),
            "confirmation_rejected".to_string(),
            format!("tool:{}", tool),
        ],
    };

    store.add_episodic_entry(entry)
}

/// 从窗口标题提取应用名
fn extract_app_name(window_title: &str) -> Option<String> {
    // 常见模式：
    // "文档.txt - 记事本" -> 记事本
    // "Google Chrome" -> Chrome
    // "微信" -> 微信
    let title = window_title.trim();

    // 尝试提取 " - " 后面的部分
    if let Some(idx) = title.rfind(" - ") {
        let app_part = title[idx + 3..].trim();
        if !app_part.is_empty() {
            return Some(app_part.to_string());
        }
    }

    // 尝试提取常见应用名
    let known_apps = [
        "Chrome",
        "Firefox",
        "Edge",
        "记事本",
        "Notepad",
        "微信",
        "WeChat",
        "VS Code",
        "Code",
        "Word",
        "Excel",
        "PowerPoint",
        "Outlook",
        "Teams",
    ];
    for app in known_apps {
        if title.to_lowercase().contains(&app.to_lowercase()) {
            return Some(app.to_string());
        }
    }

    None
}

/// 推断目标类型
fn infer_target_kind(entities: &[KeyEntity]) -> String {
    for entity in entities {
        match entity.entity_type.as_str() {
            "window" => return "window".to_string(),
            "element" => return "element".to_string(),
            "file" => return "file".to_string(),
            _ => {}
        }
    }
    "app".to_string()
}


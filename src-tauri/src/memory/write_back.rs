//! Memory Write-back - 任务完成后的记忆写入
//!
//! 负责在任务完成后将经验写入各类 memory。

use super::store::MemoryStore;
use super::types::{
    now_millis, EpisodicEntry, FrequentPath, KeyEntity, MemoryStatus, MetaPreference,
    ProceduralEntry, RuntimeContextDigest, SemanticEntry, StableWindowFeatures,
    WriteBackRequest,
};
use serde_json::json;

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

/// 从普通对话写回长期记忆
pub fn write_back_conversation_turn(
    store: &MemoryStore,
    user_input: &str,
    _assistant_reply: &str,
) -> Result<(), String> {
    let user_input = user_input.trim();
    if user_input.is_empty() {
        return Ok(());
    }

    if let Some(content) = extract_forget_content(user_input) {
        let _ = store.forget_semantic_entries(&content)?;
        store.upsert_meta_preference(MetaPreference {
            id: format!("meta-{}", now_millis()),
            category: "retention".to_string(),
            preference: "explicit_forget_requests".to_string(),
            value: json!(true),
            confidence: 0.9,
            created_at: now_millis(),
            updated_at: now_millis(),
            explicit: true,
            ttl: None,
            status: MemoryStatus::Active,
            conflict_group: None,
        })?;
    }

    if let Some(content) = extract_remember_content(user_input) {
        let now = now_millis();
        store.upsert_semantic_entry(build_semantic_entry(
            now,
            semantic_memory_key_for_explicit_content(&content),
            summarize_topic(&content),
            content.clone(),
            "user_fact".to_string(),
            0.95,
            vec![
                "conversation".to_string(),
                "explicit_memory".to_string(),
                "user_fact".to_string(),
            ],
            true,
            1,
            None,
        ))?;
        store.upsert_meta_preference(MetaPreference {
            id: format!("meta-{}", now + 1),
            category: "retention".to_string(),
            preference: "respect_explicit_remember_requests".to_string(),
            value: json!(true),
            confidence: 0.95,
            created_at: now,
            updated_at: now,
            explicit: true,
            ttl: None,
            status: MemoryStatus::Active,
            conflict_group: None,
        })?;
    }

    if let Some(language) = extract_reply_language_preference(user_input) {
        update_profile_language_preference(store, language)?;
    }

    if let Some(style) = extract_reply_style_preference(user_input) {
        update_profile_reply_style(store, style)?;
    }

    if let Some(alias) = extract_user_alias(user_input) {
        let now = now_millis();
        store.upsert_semantic_entry(build_semantic_entry(
            now,
            "user_alias".to_string(),
            "用户称呼".to_string(),
            format!("用户希望被称呼为 {}", alias),
            "user_preference".to_string(),
            0.9,
            vec![
                "conversation".to_string(),
                "user_alias".to_string(),
                "user_preference".to_string(),
            ],
            true,
            1,
            None,
        ))?;
        store.upsert_meta_preference(MetaPreference {
            id: format!("meta-{}", now + 2),
            category: "conversation".to_string(),
            preference: "user_alias".to_string(),
            value: json!(alias),
            confidence: 0.9,
            created_at: now,
            updated_at: now,
            explicit: true,
            ttl: None,
            status: MemoryStatus::Active,
            conflict_group: None,
        })?;
    }

    if let Some(candidate) = extract_candidate_user_fact(user_input) {
        let now = now_millis();
        store.upsert_semantic_entry(build_semantic_entry(
            now,
            candidate.memory_key,
            candidate.topic,
            candidate.knowledge,
            candidate.source_type,
            candidate.confidence,
            candidate.tags,
            false,
            1,
            Some(now + 30 * 24 * 3600 * 1000),
        ))?;
    }

    Ok(())
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

fn summarize_topic(content: &str) -> String {
    let text = content.trim().trim_matches(|ch: char| "。！!？?，,".contains(ch));
    let max_chars = 24;
    let topic: String = text.chars().take(max_chars).collect();
    if topic.is_empty() {
        "用户记忆".to_string()
    } else {
        topic
    }
}

fn extract_remember_content(input: &str) -> Option<String> {
    if looks_like_memory_status_query(input) {
        return None;
    }

    let prefixes = [
        "请记住",
        "帮我记住",
        "请帮我记住",
        "记住",
        "记一下",
        "帮我记一下",
        "别忘了",
    ];

    for prefix in prefixes {
        if let Some(rest) = strip_prefix_ci(input, prefix) {
            let content = clean_memory_content(rest);
            if !content.is_empty() {
                return Some(content);
            }
        }
    }

    None
}

fn extract_forget_content(input: &str) -> Option<String> {
    let prefixes = ["忘掉", "忘记", "不要记住", "别记住", "别再记", "忽略刚才"];
    for prefix in prefixes {
        if let Some(rest) = strip_prefix_ci(input, prefix) {
            let content = clean_memory_content(rest);
            if !content.is_empty() {
                return Some(content);
            }
        }
    }
    None
}

fn extract_reply_language_preference(input: &str) -> Option<&'static str> {
    if input.contains("中文回复") || input.contains("用中文") || input.contains("默认中文") {
        return Some("zh-CN");
    }
    if input.contains("英文回复")
        || input.contains("英语回复")
        || input.contains("用英文")
        || input.contains("默认英文")
    {
        return Some("en-US");
    }
    None
}

fn extract_reply_style_preference(input: &str) -> Option<&'static str> {
    if input.contains("简洁") || input.contains("简短") {
        return Some("concise");
    }
    if input.contains("详细") || input.contains("展开") {
        return Some("detailed");
    }
    if input.contains("正式") || input.contains("专业一点") {
        return Some("formal");
    }
    if input.contains("口语化") || input.contains("自然一点") {
        return Some("casual");
    }
    None
}

fn extract_user_alias(input: &str) -> Option<String> {
    let markers = ["叫我", "称呼我为", "你可以叫我"];
    for marker in markers {
        if let Some(index) = input.find(marker) {
            let rest = clean_memory_content(&input[index + marker.len()..]);
            let alias = rest
                .split(|ch: char| "，。！？!?, ".contains(ch))
                .next()
                .unwrap_or("")
                .trim();
            if !alias.is_empty() {
                return Some(alias.to_string());
            }
        }
    }
    None
}

fn update_profile_language_preference(store: &MemoryStore, language: &str) -> Result<(), String> {
    let now = now_millis();
    store.update_profile(|profile| {
        profile.language_style.preferred_language = language.to_string();
        if profile.created_at == 0 {
            profile.created_at = now;
        }
    })?;

    store.upsert_meta_preference(MetaPreference {
        id: format!("meta-{}", now),
        category: "reply".to_string(),
        preference: "preferred_language".to_string(),
        value: json!(language),
        confidence: 0.9,
        created_at: now,
        updated_at: now,
        explicit: true,
        ttl: None,
        status: MemoryStatus::Active,
        conflict_group: None,
    })
}

fn update_profile_reply_style(store: &MemoryStore, style: &str) -> Result<(), String> {
    let now = now_millis();
    store.update_profile(|profile| {
        profile.language_style.reply_style = style.to_string();
        if profile.created_at == 0 {
            profile.created_at = now;
        }
    })?;

    store.upsert_meta_preference(MetaPreference {
        id: format!("meta-{}", now),
        category: "reply".to_string(),
        preference: "reply_style".to_string(),
        value: json!(style),
        confidence: 0.85,
        created_at: now,
        updated_at: now,
        explicit: true,
        ttl: None,
        status: MemoryStatus::Active,
        conflict_group: None,
    })
}

fn looks_like_memory_status_query(input: &str) -> bool {
    input.contains("还记得")
        || input.contains("记得吗")
        || input.contains("记住了吗")
        || input.contains("你记得")
}

fn clean_memory_content(input: &str) -> String {
    input
        .trim()
        .trim_start_matches(|ch: char| ch == ':' || ch == '：' || ch == '，' || ch == ',')
        .trim()
        .trim_end_matches(|ch: char| ch == '。' || ch == '！' || ch == '!' )
        .trim()
        .to_string()
}

fn strip_prefix_ci<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    input.trim().strip_prefix(prefix)
}

struct CandidateSemanticFact {
    memory_key: String,
    topic: String,
    knowledge: String,
    source_type: String,
    confidence: f64,
    tags: Vec<String>,
}

fn extract_candidate_user_fact(input: &str) -> Option<CandidateSemanticFact> {
    if extract_remember_content(input).is_some()
        || extract_forget_content(input).is_some()
        || extract_reply_language_preference(input).is_some()
        || extract_reply_style_preference(input).is_some()
        || extract_user_alias(input).is_some()
    {
        return None;
    }

    let trimmed = input.trim();
    if let Some(content) = strip_prefix_ci(trimmed, "我喜欢") {
        let content = clean_memory_content(content);
        if !content.is_empty() {
            return Some(CandidateSemanticFact {
                memory_key: format!("user_like::{}", normalize_memory_key(&content)),
                topic: format!("用户偏好：{}", summarize_topic(&content)),
                knowledge: format!("用户喜欢 {}", content),
                source_type: "user_fact_candidate".to_string(),
                confidence: 0.45,
                tags: vec!["conversation".to_string(), "candidate".to_string(), "preference".to_string()],
            });
        }
    }

    for prefix in ["我常用", "我一般用", "我主要用"] {
        if let Some(content) = strip_prefix_ci(trimmed, prefix) {
            let content = clean_memory_content(content);
            if !content.is_empty() {
                return Some(CandidateSemanticFact {
                    memory_key: format!("user_tooling::{}", normalize_memory_key(&content)),
                    topic: format!("用户常用：{}", summarize_topic(&content)),
                    knowledge: format!("用户常用 {}", content),
                    source_type: "user_fact_candidate".to_string(),
                    confidence: 0.5,
                    tags: vec!["conversation".to_string(), "candidate".to_string(), "tooling".to_string()],
                });
            }
        }
    }

    for prefix in ["我的项目目录在", "我的工作目录在", "我的项目在"] {
        if let Some(content) = strip_prefix_ci(trimmed, prefix) {
            let content = clean_memory_content(content);
            if !content.is_empty() {
                return Some(CandidateSemanticFact {
                    memory_key: "user_project_path".to_string(),
                    topic: format!("用户目录：{}", summarize_topic(&content)),
                    knowledge: format!("用户的目录信息为 {}", content),
                    source_type: "user_fact_candidate".to_string(),
                    confidence: 0.55,
                    tags: vec!["conversation".to_string(), "candidate".to_string(), "path".to_string()],
                });
            }
        }
    }

    None
}

fn build_semantic_entry(
    now: u64,
    memory_key: String,
    topic: String,
    knowledge: String,
    source_type: String,
    confidence: f64,
    tags: Vec<String>,
    explicit: bool,
    mention_count: u32,
    ttl: Option<u64>,
) -> SemanticEntry {
    SemanticEntry {
        id: format!("sem-{}", now),
        memory_key,
        topic,
        knowledge,
        source_type,
        confidence,
        created_at: now,
        updated_at: now,
        tags,
        explicit,
        mention_count,
        ttl,
        status: MemoryStatus::Active,
        conflict_group: None,
    }
}

fn semantic_memory_key_for_explicit_content(content: &str) -> String {
    if content.contains("项目目录") || content.contains("工作目录") {
        return "user_project_path".to_string();
    }
    if content.contains("喜欢") {
        return format!("user_like::{}", normalize_memory_key(content));
    }
    if content.contains("常用") || content.contains("主要用") {
        return format!("user_tooling::{}", normalize_memory_key(content));
    }
    format!("user_fact::{}", normalize_memory_key(content))
}

fn normalize_memory_key(input: &str) -> String {
    input
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

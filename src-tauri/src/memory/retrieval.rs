//! Memory Retrieval - 检索和排序
//!
//! 根据当前任务上下文检索相关记忆，并按相关性排序。

use super::types::{
    now_millis, EpisodicEntry, EpisodicMemory, EpisodeSummary, MemoryQuery, MemorySummary,
    MemoryStatus, MemoryType, MetaMemory, MetaPreference, MetaSummary, PolicyMemory, PolicySuggestion,
    PolicySummary, ProceduralEntry, ProceduralMemory, ProcedureSummary, ProfileHints,
    ProfileMemory, SemanticEntry, SemanticMemory, SemanticSummary,
};

/// 检索相关的 Episodic Memory
pub fn retrieve_episodes(
    episodic: &EpisodicMemory,
    query: &MemoryQuery,
) -> Vec<(EpisodicEntry, f64)> {
    if !allows_memory_type(query, MemoryType::Episodic) {
        return Vec::new();
    }

    let mut results: Vec<(EpisodicEntry, f64)> = Vec::new();

    for entry in &episodic.entries {
        if let Some(min_confidence) = query.min_confidence {
            let inferred_confidence = if entry.final_status == "completed" { 0.85 } else { 0.6 };
            if inferred_confidence < min_confidence {
                continue;
            }
        }
        let score = compute_episode_relevance(entry, query);
        if score > 0.1 {
            results.push((entry.clone(), score));
        }
    }

    // 按分数降序排序
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // 限制返回数量
    let limit = if query.limit > 0 { query.limit } else { 5 };
    results.truncate(limit);

    results
}

/// 计算 Episode 相关性分数
fn compute_episode_relevance(entry: &EpisodicEntry, query: &MemoryQuery) -> f64 {
    let mut score = 0.0;

    // 1. Goal 相似度 (最重要)
    if let Some(ref goal) = query.goal {
        let goal_sim = text_similarity(&entry.goal, goal);
        score += goal_sim * 0.4;
    }

    // 2. Intent 匹配
    if let Some(ref intent) = query.intent {
        if entry.intent.to_lowercase() == intent.to_lowercase() {
            score += 0.2;
        }
    }

    // 3. 窗口标题匹配
    if let Some(ref window_title) = query.window_title {
        if let Some(ref digest_title) = entry.runtime_context_digest.active_window_title {
            let title_sim = text_similarity(digest_title, window_title);
            score += title_sim * 0.15;
        }
    }

    // 4. 标签匹配
    if !query.tags.is_empty() {
        let tag_matches = query
            .tags
            .iter()
            .filter(|t| entry.tags.contains(t))
            .count();
        let tag_score = tag_matches as f64 / query.tags.len() as f64;
        score += tag_score * 0.1;
    }

    // 5. 时间衰减 (最近的更相关)
    let age_hours = (now_millis() - entry.timestamp) as f64 / (1000.0 * 60.0 * 60.0);
    let recency_factor = 1.0 / (1.0 + age_hours / 24.0); // 24小时内权重较高
    score *= 0.7 + 0.3 * recency_factor;

    // 6. 成功任务加分，失败任务不加但也不减太多
    if entry.final_status == "completed" {
        score *= 1.1;
    } else if entry.final_status == "failed" {
        score *= 0.9;
    }

    score
}

/// 检索相关的 Procedural Memory
pub fn retrieve_procedures(
    procedural: &ProceduralMemory,
    query: &MemoryQuery,
) -> Vec<(ProceduralEntry, f64)> {
    if !allows_memory_type(query, MemoryType::Procedural) {
        return Vec::new();
    }

    let mut results: Vec<(ProceduralEntry, f64)> = Vec::new();

    for entry in &procedural.procedures {
        if let Some(min_confidence) = query.min_confidence {
            if entry.confidence < min_confidence {
                continue;
            }
        }
        let score = compute_procedure_relevance(entry, query);
        if score > 0.1 {
            results.push((entry.clone(), score));
        }
    }

    // 按分数降序排序
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // 限制返回数量
    let limit = if query.limit > 0 { query.limit } else { 3 };
    results.truncate(limit);

    results
}

/// 计算 Procedure 相关性分数
fn compute_procedure_relevance(entry: &ProceduralEntry, query: &MemoryQuery) -> f64 {
    let mut score = 0.0;

    // 1. 目标模式匹配
    if let Some(ref goal) = query.goal {
        let pattern_sim = text_similarity(&entry.target_pattern, goal);
        score += pattern_sim * 0.3;
    }

    // 2. 窗口标题匹配
    if let Some(ref window_title) = query.window_title {
        if let Some(ref features) = entry.stable_window_features {
            let title_sim = text_similarity(&features.title_pattern, window_title);
            score += title_sim * 0.3;
        }
    }

    // 3. 应用名匹配
    if let Some(ref app_name) = query.app_name {
        if entry.target_pattern.to_lowercase().contains(&app_name.to_lowercase()) {
            score += 0.2;
        }
    }

    // 4. 置信度因子
    score *= 0.5 + 0.5 * entry.confidence;

    // 5. 成功率因子
    let total = entry.success_count + entry.failure_count;
    if total > 0 {
        let success_rate = entry.success_count as f64 / total as f64;
        score *= 0.6 + 0.4 * success_rate;
    }

    // 6. 时间衰减
    let age_hours = (now_millis() - entry.last_verified_at) as f64 / (1000.0 * 60.0 * 60.0);
    let recency_factor = 1.0 / (1.0 + age_hours / 168.0); // 一周内权重较高
    score *= 0.8 + 0.2 * recency_factor;

    score
}

/// 检索适用的 Policy Suggestions
pub fn retrieve_policies(policy: &PolicyMemory, scope_prefix: &str) -> Vec<PolicySuggestion> {
    policy
        .suggestions
        .iter()
        .filter(|s| {
            s.scope == "global"
                || s.scope.starts_with(scope_prefix)
                || scope_prefix.starts_with(&s.scope)
        })
        .cloned()
        .collect()
}

/// 检索相关的 Semantic Memory
pub fn retrieve_semantic(
    semantic: &SemanticMemory,
    query: &MemoryQuery,
) -> Vec<(SemanticEntry, f64)> {
    if !allows_memory_type(query, MemoryType::Semantic) {
        return Vec::new();
    }

    let mut results: Vec<(SemanticEntry, f64)> = Vec::new();

    for entry in &semantic.entries {
        if entry.status != MemoryStatus::Active {
            continue;
        }
        if entry.ttl.map(|ttl| now_millis() > ttl).unwrap_or(false) {
            continue;
        }
        if !entry.explicit && entry.mention_count < 2 {
            continue;
        }
        if let Some(min_confidence) = query.min_confidence {
            if entry.confidence < min_confidence {
                continue;
            }
        }
        let score = compute_semantic_relevance(entry, query);
        if score > 0.08 {
            results.push((entry.clone(), score));
        }
    }

    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let limit = if query.limit > 0 { query.limit.min(3) } else { 3 };
    results.truncate(limit);
    results
}

/// 检索相关的 Meta Preferences
pub fn retrieve_meta(meta: &MetaMemory, query: &MemoryQuery) -> Vec<MetaPreference> {
    if !allows_memory_type(query, MemoryType::Meta) {
        return Vec::new();
    }

    let goal = query.goal.as_deref().unwrap_or_default().to_lowercase();
    let wants_memory_controls = goal.contains("记住")
        || goal.contains("忘")
        || goal.contains("默认")
        || goal.contains("回复")
        || goal.contains("称呼");

    meta.preferences
        .iter()
        .filter(|entry| entry.status == MemoryStatus::Active)
        .filter(|entry| entry.ttl.map(|ttl| now_millis() <= ttl).unwrap_or(true))
        .filter(|entry| entry.confidence >= 0.4)
        .filter(|entry| {
            wants_memory_controls
                || matches!(entry.category.as_str(), "retention" | "reply" | "conversation")
        })
        .cloned()
        .take(4)
        .collect()
}

fn allows_memory_type(query: &MemoryQuery, memory_type: MemoryType) -> bool {
    query.memory_types.is_empty() || query.memory_types.contains(&memory_type)
}

/// 构建 Memory Summary (用于 prompt 注入)
pub fn build_memory_summary(
    profile: &ProfileMemory,
    episodic: &EpisodicMemory,
    procedural: &ProceduralMemory,
    policy: &PolicyMemory,
    semantic: &SemanticMemory,
    meta: &MetaMemory,
    query: &MemoryQuery,
) -> MemorySummary {
    // 1. 检索相关 episodes
    let episodes = retrieve_episodes(episodic, query);
    let relevant_episodes: Vec<EpisodeSummary> = episodes
        .into_iter()
        .take(3)
        .map(|(entry, score)| {
            let key_insight = if entry.final_status == "completed" {
                format!("成功使用 {} 完成", entry.used_tools.join("+"))
            } else {
                format!(
                    "失败于 {:?}: {:?}",
                    entry.failure_stage, entry.failure_reason_code
                )
            };
            EpisodeSummary {
                goal: entry.goal,
                final_status: entry.final_status,
                key_insight,
                relevance_score: score,
            }
        })
        .collect();

    // 2. 检索相关 procedures
    let procedures = retrieve_procedures(procedural, query);
    let relevant_procedures: Vec<ProcedureSummary> = procedures
        .into_iter()
        .take(2)
        .map(|(entry, _)| {
            let total = entry.success_count + entry.failure_count;
            let success_rate = if total > 0 {
                entry.success_count as f64 / total as f64
            } else {
                0.0
            };
            ProcedureSummary {
                target_pattern: entry.target_pattern,
                preferred_approach: entry.preferred_tool_sequence.join(" -> "),
                confidence: entry.confidence,
                success_rate,
            }
        })
        .collect();

    // 3. 检索适用的 policies
    let scope_prefix = query.app_name.as_deref().unwrap_or("global");
    let policies = if allows_memory_type(query, MemoryType::Policy) {
        retrieve_policies(policy, scope_prefix)
    } else {
        Vec::new()
    };
    let active_policies: Vec<PolicySummary> = policies
        .into_iter()
        .filter(|s| s.approved || s.confidence > 0.7)
        .take(5)
        .map(|s| PolicySummary {
            suggestion_type: s.suggestion_type,
            value: s.value,
            scope: s.scope,
        })
        .collect();

    // 4. 检索 semantic context
    let semantic_context: Vec<SemanticSummary> = retrieve_semantic(semantic, query)
        .into_iter()
        .map(|(entry, score)| SemanticSummary {
            topic: entry.topic,
            knowledge: entry.knowledge,
            relevance_score: score,
        })
        .collect();

    // 5. 检索 meta preferences
    let meta_preferences: Vec<MetaSummary> = retrieve_meta(meta, query)
        .into_iter()
        .map(|entry| MetaSummary {
            category: entry.category,
            preference: entry.preference,
            value: meta_value_to_string(&entry.value),
            confidence: entry.confidence,
        })
        .collect();

    // 6. Profile hints
    let profile_hints = ProfileHints {
        preferred_apps: profile
            .preferred_apps
            .iter()
            .filter(|(_, count)| **count > 2)
            .map(|(app, _)| app.clone())
            .take(5)
            .collect(),
        risk_preference: if profile.risk_preference_low_level_only {
            "conservative".to_string()
        } else {
            "balanced".to_string()
        },
    };

    MemorySummary {
        relevant_episodes,
        relevant_procedures,
        active_policies,
        semantic_context,
        meta_preferences,
        profile_hints,
    }
}

/// 渲染 Memory Summary 为 prompt 文本
pub fn render_memory_summary_for_prompt(summary: &MemorySummary) -> String {
    let mut lines = Vec::new();

    lines.push("## Memory Context".to_string());

    // Profile
    if !summary.profile_hints.preferred_apps.is_empty() {
        lines.push(format!(
            "用户常用应用: {}",
            summary.profile_hints.preferred_apps.join(", ")
        ));
    }
    lines.push(format!(
        "风险偏好: {}",
        summary.profile_hints.risk_preference
    ));

    // Relevant episodes
    if !summary.relevant_episodes.is_empty() {
        lines.push("\n### 相关历史经验".to_string());
        for ep in &summary.relevant_episodes {
            lines.push(format!(
                "- [{}] \"{}\": {} (相关度 {:.2})",
                ep.final_status, ep.goal, ep.key_insight, ep.relevance_score
            ));
        }
    }

    // Relevant procedures
    if !summary.relevant_procedures.is_empty() {
        lines.push("\n### 已知稳定路径".to_string());
        for proc in &summary.relevant_procedures {
            lines.push(format!(
                "- \"{}\": {} (置信度 {:.2}, 成功率 {:.0}%)",
                proc.target_pattern,
                proc.preferred_approach,
                proc.confidence,
                proc.success_rate * 100.0
            ));
        }
    }

    // Semantic context
    if !summary.semantic_context.is_empty() {
        lines.push("\n### 语义记忆".to_string());
        for item in &summary.semantic_context {
            lines.push(format!(
                "- {}: {} (相关度 {:.2})",
                item.topic, item.knowledge, item.relevance_score
            ));
        }
    }

    // Meta preferences
    if !summary.meta_preferences.is_empty() {
        lines.push("\n### 记忆与回复偏好".to_string());
        for item in &summary.meta_preferences {
            lines.push(format!(
                "- [{}] {} = {} (置信度 {:.2})",
                item.category, item.preference, item.value, item.confidence
            ));
        }
    }

    // Active policies
    if !summary.active_policies.is_empty() {
        lines.push("\n### 适用策略建议".to_string());
        for pol in &summary.active_policies {
            lines.push(format!(
                "- [{}]: {} (scope: {})",
                pol.suggestion_type, pol.value, pol.scope
            ));
        }
    }

    lines.join("\n")
}

fn compute_semantic_relevance(entry: &SemanticEntry, query: &MemoryQuery) -> f64 {
    let mut score = 0.0;

    if let Some(ref goal) = query.goal {
        score += text_similarity(&entry.topic, goal) * 0.4;
        score += text_similarity(&entry.knowledge, goal) * 0.25;
    }

    if let Some(ref intent) = query.intent {
        score += text_similarity(&entry.source_type, intent) * 0.1;
        let tag_matches = entry
            .tags
            .iter()
            .filter(|tag| text_similarity(tag, intent) > 0.8)
            .count();
        if tag_matches > 0 {
            score += 0.1;
        }
    }

    if let Some(ref app_name) = query.app_name {
        score += text_similarity(&entry.topic, app_name) * 0.15;
        score += text_similarity(&entry.knowledge, app_name) * 0.1;
    }

    if !query.tags.is_empty() {
        let tag_matches = query
            .tags
            .iter()
            .filter(|tag| entry.tags.iter().any(|entry_tag| entry_tag == *tag))
            .count();
        if tag_matches > 0 {
            score += 0.1 * tag_matches as f64 / query.tags.len() as f64;
        }
    }

    let age_hours = (now_millis().saturating_sub(entry.updated_at)) as f64 / 3_600_000.0;
    let recency_factor = 1.0 / (1.0 + age_hours / 168.0);
    score *= 0.6 + 0.25 * entry.confidence + 0.15 * recency_factor;
    score
}

fn meta_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

/// 简单文本相似度 (基于词重叠 Jaccard 系数)
fn text_similarity(a: &str, b: &str) -> f64 {
    // 先绑定到变量，避免临时值生命周期问题
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    let a_words: std::collections::HashSet<&str> = a_lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .collect();
    let b_words: std::collections::HashSet<&str> = b_lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .collect();

    if a_words.is_empty() || b_words.is_empty() {
        return 0.0;
    }

    let intersection = a_words.intersection(&b_words).count();
    let union = a_words.union(&b_words).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

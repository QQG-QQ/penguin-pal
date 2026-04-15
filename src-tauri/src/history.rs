use chrono::Local;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

use crate::app_state::now_millis;

const INPUT_HISTORY_LIMIT: usize = 50;
const HISTORY_ROOT: &str = "history";
const INPUT_HISTORY_FILE: &str = "input/history.json";
const DAILY_CURRENT_FILE: &str = "daily/current.json";
const ARCHIVE_DIR: &str = "archive";
const ARCHIVE_RECALL_DEFAULT_LIMIT: usize = 3;
const ARCHIVE_RECALL_MIN_SCORE: f64 = 0.22;
const ARCHIVE_RECALL_MIN_SIGNAL_CHARS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyHistoryEntry {
    pub id: String,
    pub timestamp: u64,
    pub user_input: String,
    pub assistant_reply: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct InputHistoryFile {
    items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DailyConversationLog {
    date: String,
    entries: Vec<ReplyHistoryEntry>,
}

#[derive(Debug, Clone)]
struct ReplyHistorySearchItem {
    day_key: String,
    entry: ReplyHistoryEntry,
}

#[derive(Debug, Clone)]
struct ReplyHistoryMatch {
    day_key: String,
    entry: ReplyHistoryEntry,
    score: f64,
}

fn history_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(HISTORY_ROOT);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    Ok(root)
}

fn input_history_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(history_root(app)?.join(INPUT_HISTORY_FILE))
}

fn daily_current_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(history_root(app)?.join(DAILY_CURRENT_FILE))
}

fn archive_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(history_root(app)?.join(ARCHIVE_DIR))
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    fs::create_dir_all(parent).map_err(|error| error.to_string())
}

fn today_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn build_backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("history");
    path.with_file_name(format!("{file_name}.corrupt-{}.bak", now_millis()))
}

fn move_to_backup(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let backup = build_backup_path(path);
    fs::rename(path, backup).map_err(|error| error.to_string())
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };

    match serde_json::from_str::<T>(&content) {
        Ok(value) => Ok(Some(value)),
        Err(_) => {
            move_to_backup(path)?;
            Ok(None)
        }
    }
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    ensure_parent(path)?;
    let content = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

fn load_input_file(app: &AppHandle) -> Result<InputHistoryFile, String> {
    let path = input_history_path(app)?;
    Ok(read_json_file::<InputHistoryFile>(&path)?.unwrap_or_default())
}

fn load_today_log(app: &AppHandle) -> Result<Option<DailyConversationLog>, String> {
    let path = daily_current_path(app)?;
    read_json_file::<DailyConversationLog>(&path)
}

fn archive_existing_log(app: &AppHandle, log: &DailyConversationLog) -> Result<(), String> {
    if log.entries.is_empty() || log.date.trim().is_empty() {
        return Ok(());
    }

    let archive_path = archive_dir(app)?.join(format!("{}.json", log.date));
    let mut archive_log = read_json_file::<DailyConversationLog>(&archive_path)?.unwrap_or(
        DailyConversationLog {
            date: log.date.clone(),
            entries: vec![],
        },
    );
    archive_log.entries.extend(log.entries.clone());
    write_json_file(&archive_path, &archive_log)
}

fn rotate_daily_log_if_needed(app: &AppHandle) -> Result<(), String> {
    let path = daily_current_path(app)?;
    let Some(log) = load_today_log(app)? else {
        return Ok(());
    };

    if log.date == today_key() {
        return Ok(());
    }

    archive_existing_log(app, &log)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn build_today_log() -> DailyConversationLog {
    DailyConversationLog {
        date: today_key(),
        entries: vec![],
    }
}

fn load_reply_history_search_items(app: &AppHandle) -> Result<Vec<ReplyHistorySearchItem>, String> {
    rotate_daily_log_if_needed(app)?;

    let mut items = Vec::new();

    if let Some(log) = load_today_log(app)? {
        let day_key = if log.date.trim().is_empty() {
            today_key()
        } else {
            log.date
        };
        items.extend(log.entries.into_iter().map(|entry| ReplyHistorySearchItem {
            day_key: day_key.clone(),
            entry,
        }));
    }

    let archive_root = archive_dir(app)?;
    if !archive_root.exists() {
        return Ok(items);
    }

    let mut archive_paths = fs::read_dir(&archive_root)
        .map_err(|error| error.to_string())?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    archive_paths.sort_by(|left, right| right.cmp(left));

    for path in archive_paths {
        let Some(log) = read_json_file::<DailyConversationLog>(&path)? else {
            continue;
        };

        let day_key = if log.date.trim().is_empty() {
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("archive")
                .to_string()
        } else {
            log.date
        };

        items.extend(log.entries.into_iter().map(|entry| ReplyHistorySearchItem {
            day_key: day_key.clone(),
            entry,
        }));
    }

    Ok(items)
}

#[allow(dead_code)]
fn contains_recall_signal_legacy(query: &str) -> bool {
    let lowered = query.trim().to_lowercase();
    [
        "之前",
        "上次",
        "以前",
        "你说过",
        "我说过",
        "记得",
        "还记得",
        "刚才",
        "earlier",
        "before",
        "previous",
        "remember",
        "history",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn contains_recall_signal(query: &str) -> bool {
    let lowered = query.trim().to_lowercase();
    let chinese_needles = [
        "\u{4e4b}\u{524d}",
        "\u{4e0a}\u{6b21}",
        "\u{4ee5}\u{524d}",
        "\u{4f60}\u{8bf4}\u{8fc7}",
        "\u{6211}\u{8bf4}\u{8fc7}",
        "\u{8bb0}\u{5f97}",
        "\u{8fd8}\u{8bb0}\u{5f97}",
        "\u{521a}\u{624d}",
    ];
    let english_needles = ["earlier", "before", "previous", "remember", "history"];

    chinese_needles
        .iter()
        .chain(english_needles.iter())
        .any(|needle| lowered.contains(needle))
}

fn compact_signal_len(input: &str) -> usize {
    input.chars().filter(|ch| ch.is_alphanumeric()).count()
}

fn tokenize_words(input: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut current = String::new();

    for ch in input.chars().flat_map(|value| value.to_lowercase()) {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
            continue;
        }

        if !current.is_empty() {
            tokens.insert(std::mem::take(&mut current));
        }

        if ch.is_alphanumeric() && !ch.is_ascii() {
            tokens.insert(ch.to_string());
        }
    }

    if !current.is_empty() {
        tokens.insert(current);
    }

    tokens
}

fn tokenize_bigrams(input: &str) -> HashSet<String> {
    let compact = input
        .chars()
        .flat_map(|value| value.to_lowercase())
        .filter(|ch| ch.is_alphanumeric())
        .collect::<String>();
    let chars = compact.chars().collect::<Vec<_>>();

    if chars.is_empty() {
        return HashSet::new();
    }

    if chars.len() == 1 {
        return std::iter::once(chars[0].to_string()).collect();
    }

    chars
        .windows(2)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn jaccard_similarity(left: &HashSet<String>, right: &HashSet<String>) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let intersection = left.intersection(right).count();
    let union = left.union(right).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

fn text_recall_similarity(query: &str, candidate: &str) -> f64 {
    let word_score = jaccard_similarity(&tokenize_words(query), &tokenize_words(candidate));
    let bigram_score = jaccard_similarity(&tokenize_bigrams(query), &tokenize_bigrams(candidate));
    word_score * 0.4 + bigram_score * 0.6
}

fn compute_reply_history_match_score(
    item: &ReplyHistorySearchItem,
    query: &str,
    now: u64,
) -> f64 {
    let user_score = text_recall_similarity(query, &item.entry.user_input);
    let reply_score = text_recall_similarity(query, &item.entry.assistant_reply);
    let combined = format!("{} {}", item.entry.user_input, item.entry.assistant_reply);
    let combined_score = text_recall_similarity(query, &combined);
    let base_score = user_score * 0.5 + reply_score * 0.2 + combined_score * 0.3;

    if base_score <= 0.0 {
        return 0.0;
    }

    let recency_factor = reply_history_recency_factor(item, now);
    base_score * (0.9 + 0.1 * recency_factor)
}

fn reply_history_recency_factor(item: &ReplyHistorySearchItem, now: u64) -> f64 {
    let age_days = now.saturating_sub(item.entry.timestamp) as f64 / 86_400_000.0;
    1.0 / (1.0 + age_days / 30.0)
}

fn rank_reply_history_matches(
    items: &[ReplyHistorySearchItem],
    query: &str,
    limit: usize,
    now: u64,
) -> Vec<ReplyHistoryMatch> {
    if query.trim().is_empty() {
        return Vec::new();
    }

    if compact_signal_len(query) < ARCHIVE_RECALL_MIN_SIGNAL_CHARS && !contains_recall_signal(query)
    {
        return Vec::new();
    }

    let has_recall_signal = contains_recall_signal(query);
    let min_score = if has_recall_signal {
        ARCHIVE_RECALL_MIN_SCORE * 0.7
    } else {
        ARCHIVE_RECALL_MIN_SCORE
    };

    let mut ranked = items
        .iter()
        .filter_map(|item| {
            let mut score = compute_reply_history_match_score(item, query, now);
            if has_recall_signal && score < min_score {
                score = score.max(0.10 + 0.10 * reply_history_recency_factor(item, now));
            }
            if has_recall_signal && score <= 0.0 {
                // Queries like "之前呢" should surface a few recent snippets even without lexical overlap.
                score = 0.10 + 0.10 * reply_history_recency_factor(item, now);
            }
            if score < min_score {
                return None;
            }
            Some(ReplyHistoryMatch {
                day_key: item.day_key.clone(),
                entry: item.entry.clone(),
                score,
            })
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.entry.timestamp.cmp(&left.entry.timestamp))
    });

    ranked.truncate(limit);
    ranked
}

fn ellipsize(input: &str, max_chars: usize) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let shortened = trimmed.chars().take(max_chars).collect::<String>();
    if trimmed.chars().count() > max_chars {
        format!("{shortened}...")
    } else {
        shortened
    }
}

pub fn render_archive_recall_for_prompt(
    app: &AppHandle,
    query: &str,
    limit: usize,
) -> Result<Option<String>, String> {
    let items = load_reply_history_search_items(app)?;
    let recall_limit = if limit == 0 {
        ARCHIVE_RECALL_DEFAULT_LIMIT
    } else {
        limit
    };
    let ranked = rank_reply_history_matches(
        &items,
        query,
        recall_limit,
        now_millis(),
    );

    if ranked.is_empty() {
        return Ok(None);
    }

    let mut lines = vec![
        "## Archive Recall".to_string(),
        "Use these older verbatim snippets only when they are directly relevant.".to_string(),
    ];

    for hit in ranked {
        lines.push(format!(
            "- [{} | score {:.2}] User: {}",
            hit.day_key,
            hit.score,
            ellipsize(&hit.entry.user_input, 120)
        ));
        lines.push(format!(
            "  Assistant: {}",
            ellipsize(&hit.entry.assistant_reply, 160)
        ));
    }

    Ok(Some(lines.join("\n")))
}

pub fn prepare_storage(app: &AppHandle) -> Result<(), String> {
    rotate_daily_log_if_needed(app)
}

pub fn get_input_history(app: &AppHandle) -> Result<Vec<String>, String> {
    Ok(load_input_file(app)?.items)
}

pub fn record_input_history(app: &AppHandle, content: &str) -> Result<Vec<String>, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return get_input_history(app);
    }

    let path = input_history_path(app)?;
    let mut history = load_input_file(app)?;
    let should_append = history
        .items
        .last()
        .map(|item| item != trimmed)
        .unwrap_or(true);

    if should_append {
        history.items.push(trimmed.to_string());
    }

    if history.items.len() > INPUT_HISTORY_LIMIT {
        let extra = history.items.len() - INPUT_HISTORY_LIMIT;
        history.items.drain(0..extra);
    }

    write_json_file(&path, &history)?;
    Ok(history.items)
}

pub fn get_today_reply_history(app: &AppHandle) -> Result<Vec<ReplyHistoryEntry>, String> {
    rotate_daily_log_if_needed(app)?;
    let Some(log) = load_today_log(app)? else {
        return Ok(vec![]);
    };

    if log.date != today_key() {
        return Ok(vec![]);
    }

    Ok(log.entries)
}

pub fn record_reply_history(
    app: &AppHandle,
    user_input: &str,
    assistant_reply: &str,
) -> Result<Vec<ReplyHistoryEntry>, String> {
    let trimmed_user = user_input.trim();
    let trimmed_reply = assistant_reply.trim();

    if trimmed_user.is_empty() || trimmed_reply.is_empty() {
        return get_today_reply_history(app);
    }

    rotate_daily_log_if_needed(app)?;
    let path = daily_current_path(app)?;
    let mut log = load_today_log(app)?.unwrap_or_else(build_today_log);
    if log.date != today_key() {
        log = build_today_log();
    }

    log.entries.push(ReplyHistoryEntry {
        id: format!("reply-{}", now_millis()),
        timestamp: now_millis(),
        user_input: trimmed_user.to_string(),
        assistant_reply: trimmed_reply.to_string(),
    });

    write_json_file(&path, &log)?;
    Ok(log.entries)
}

pub fn clear_today_reply_history(app: &AppHandle) -> Result<Vec<ReplyHistoryEntry>, String> {
    rotate_daily_log_if_needed(app)?;
    let path = daily_current_path(app)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(vec![]),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(vec![]),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compact_signal_len, compute_reply_history_match_score, rank_reply_history_matches,
        ReplyHistoryEntry, ReplyHistorySearchItem,
    };

    fn item(day_key: &str, user_input: &str, assistant_reply: &str, timestamp: u64) -> ReplyHistorySearchItem {
        ReplyHistorySearchItem {
            day_key: day_key.to_string(),
            entry: ReplyHistoryEntry {
                id: format!("reply-{timestamp}"),
                timestamp,
                user_input: user_input.to_string(),
                assistant_reply: assistant_reply.to_string(),
            },
        }
    }

    #[test]
    fn compact_signal_len_counts_alphanumeric_chars() {
        assert_eq!(compact_signal_len("  "), 0);
        assert_eq!(compact_signal_len("之前"), 2);
        assert_eq!(compact_signal_len("研究习惯"), 4);
    }

    #[test]
    fn matching_entry_scores_higher_than_unrelated_entry() {
        let now = 1_750_000_000_000;
        let related = item(
            "2026-04-01",
            "记住我的投研习惯是先看结论再看反证",
            "我会优先按这个顺序帮你组织分析。",
            now - 1_000,
        );
        let unrelated = item(
            "2026-04-02",
            "今天晚饭想吃什么",
            "可以做个清淡一点的菜单。",
            now - 1_000,
        );

        let related_score = compute_reply_history_match_score(&related, "投研习惯", now);
        let unrelated_score = compute_reply_history_match_score(&unrelated, "投研习惯", now);

        assert!(related_score > unrelated_score);
        assert!(related_score > 0.0);
    }

    #[test]
    fn recall_signal_allows_short_history_queries() {
        let now = 1_750_000_000_000;
        let items = vec![item(
            "2026-04-01",
            "你之前让我默认用中文简洁回复",
            "我会保持中文和简洁风格。",
            now - 1_000,
        )];

        let ranked = rank_reply_history_matches(&items, "之前", 3, now);
        assert_eq!(ranked.len(), 1);
    }

    #[test]
    fn recall_signal_allows_short_history_queries_ascii() {
        let now = 1_750_000_000_000;
        let items = vec![item(
            "2026-04-01",
            "Keep replies concise and in Chinese by default.",
            "I will keep the default reply style concise and in Chinese.",
            now - 1_000,
        )];

        let ranked = rank_reply_history_matches(&items, "before", 3, now);
        assert_eq!(ranked.len(), 1);
    }

    #[test]
    fn short_queries_without_signal_do_not_trigger_archive_recall() {
        let now = 1_750_000_000_000;
        let items = vec![item(
            "2026-04-01",
            "你之前让我默认用中文简洁回复",
            "我会保持中文和简洁风格。",
            now - 1_000,
        )];

        let ranked = rank_reply_history_matches(&items, "你好", 3, now);
        assert!(ranked.is_empty());
    }
}

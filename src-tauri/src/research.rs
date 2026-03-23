use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::{
    ai::{guardrails, provider},
    app_state::{now_millis, save, ChatMessage, ProviderKind, ResearchConfig, RuntimeState},
    codex_runtime::resolve_for_app,
    memory::{generate_id, MemoryService, MemoryStatus, MemoryStore, MetaPreference, SemanticEntry},
    security::policy,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchBriefSection {
    pub title: String,
    pub summary: String,
    pub bullets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchBriefAlert {
    pub id: String,
    pub severity: String,
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchFundQuote {
    pub code: String,
    pub name: String,
    pub estimate_nav: Option<f64>,
    pub previous_nav: Option<f64>,
    pub change_percent: Option<f64>,
    pub estimate_time: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchBriefSnapshot {
    pub generated_at: u64,
    pub day_key: String,
    pub enabled: bool,
    pub title: String,
    pub summary: String,
    pub sections: Vec<ResearchBriefSection>,
    pub alerts: Vec<ResearchBriefAlert>,
    pub fund_quotes: Vec<ResearchFundQuote>,
    pub memory_hints: Vec<String>,
    pub alert_fingerprint: String,
    pub has_updates: bool,
    pub startup_popup_due: bool,
    pub update_summary: Option<String>,
    pub analysis_status: String,
    pub analysis_provider_label: Option<String>,
    pub analysis_result: Option<String>,
    pub analysis_notice: Option<String>,
}

#[derive(Debug, Clone)]
struct ResearchAiAnalysis {
    status: String,
    provider_label: Option<String>,
    result: Option<String>,
    notice: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FundEstimatePayload {
    fundcode: Option<String>,
    name: Option<String>,
    dwjz: Option<String>,
    gsz: Option<String>,
    gszzl: Option<String>,
    gztime: Option<String>,
}

pub fn normalize_config(config: &ResearchConfig) -> ResearchConfig {
    let mut next = config.clone();
    next.watchlist = normalize_list(&config.watchlist);
    next.funds = normalize_list(&config.funds);
    next.themes = {
        let normalized = normalize_list(&config.themes);
        if normalized.is_empty() {
            vec![
                "地缘政治".to_string(),
                "财报".to_string(),
                "基金风格".to_string(),
            ]
        } else {
            normalized
        }
    };
    next.habit_notes = config.habit_notes.trim().to_string();
    next.decision_framework = if config.decision_framework.trim().is_empty() {
        "先看结论和证据，再看反证、风险、失效条件、跟踪指标，最后才决定是否继续研究。"
            .to_string()
    } else {
        config.decision_framework.trim().to_string()
    };
    next
}

pub async fn build_brief(
    app: &AppHandle,
    runtime: &RuntimeState,
) -> Result<ResearchBriefSnapshot, String> {
    let config = normalize_config(&runtime.research);
    let generated_at = now_millis();
    let day_key = local_day_key(generated_at);

    if !config.enabled {
        return Ok(ResearchBriefSnapshot {
            generated_at,
            day_key,
            enabled: false,
            title: "本地投研模式未启用".to_string(),
            summary: "你可以在设置里打开投研模式，自定义自选标的、基金、主题和决策框架。".to_string(),
            sections: Vec::new(),
            alerts: vec![ResearchBriefAlert {
                id: generate_id("research_alert"),
                severity: "info".to_string(),
                title: "投研模式未开启".to_string(),
                summary: "开启后，桌宠会在独立弹窗里展示每日研究简报，并用长期记忆记住你的投资习惯。".to_string(),
            }],
            fund_quotes: Vec::new(),
            memory_hints: Vec::new(),
            alert_fingerprint: String::new(),
            has_updates: false,
            startup_popup_due: false,
            update_summary: None,
            analysis_status: "disabled".to_string(),
            analysis_provider_label: None,
            analysis_result: None,
            analysis_notice: Some(
                "投研模式未启用，因此不会生成 AI 分析结果。".to_string(),
            ),
        });
    }

    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = MemoryService::new(&app_data);
    let memory_hints = collect_investment_memory_hints(&memory_service);
    let fund_quotes = fetch_fund_quotes(&config, runtime.provider.allow_network, generated_at).await;

    let watchlist_label = if config.watchlist.is_empty() {
        "未配置股票/ETF 自选".to_string()
    } else {
        config.watchlist.join("、")
    };
    let funds_label = if config.funds.is_empty() {
        "未配置基金观察池".to_string()
    } else {
        config.funds.join("、")
    };

    let mut sections = vec![
        ResearchBriefSection {
            title: "今日研究范围".to_string(),
            summary: format!(
                "围绕 {} 个股票/ETF、{} 个基金与 {} 个主题做本地研究。",
                config.watchlist.len(),
                config.funds.len(),
                config.themes.len()
            ),
            bullets: vec![
                format!("股票 / ETF 自选：{watchlist_label}"),
                format!("基金观察池：{funds_label}"),
                format!("主题雷达：{}", config.themes.join("、")),
            ],
        },
        ResearchBriefSection {
            title: "财报拆解逻辑".to_string(),
            summary: "先按统一财报框架看利润质量、现金流和管理层指引，再决定要不要继续深挖。".to_string(),
            bullets: build_watchlist_bullets(&config),
        },
        ResearchBriefSection {
            title: "基金涨幅快照".to_string(),
            summary: "先看你当前基金池的估算涨跌，再决定今天是做风格对比还是做仓位复核。".to_string(),
            bullets: build_fund_quote_bullets(&fund_quotes),
        },
        ResearchBriefSection {
            title: "基金风格比较".to_string(),
            summary: "统一用风格、仓位、换手、回撤和拥挤度来比基金，而不是只看短期收益率。".to_string(),
            bullets: build_fund_bullets(&config),
        },
        ResearchBriefSection {
            title: "地缘与新闻增强分析".to_string(),
            summary: "本地模式会先围绕主题做影响路径拆解，后续再叠加实时外部数据源。".to_string(),
            bullets: build_theme_bullets(&config),
        },
        ResearchBriefSection {
            title: "决策框架".to_string(),
            summary: "把判断写成可复查的框架，避免只凭情绪做结论。".to_string(),
            bullets: config
                .decision_framework
                .split(['\n', '。', ';', '；'])
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| item.to_string())
                .collect(),
        },
        ResearchBriefSection {
            title: "今日观察清单".to_string(),
            summary: "把今天最值得盯住的变量提前列出来，方便你快速过一遍研究顺序。".to_string(),
            bullets: build_observation_checklist(&config, &memory_hints),
        },
        ResearchBriefSection {
            title: "增强分析执行面板".to_string(),
            summary: "把财报、基金风格、地缘主题和你的研究习惯串成今天应该先看的动作。".to_string(),
            bullets: build_analysis_actions(&config, &memory_hints),
        },
    ];

    if !config.habit_notes.is_empty() || !memory_hints.is_empty() {
        let mut bullets = Vec::new();
        if !config.habit_notes.is_empty() {
            bullets.push(format!("当前记录的交易/研究习惯：{}", config.habit_notes));
        }
        bullets.extend(memory_hints.iter().cloned());
        sections.push(ResearchBriefSection {
            title: "长期记忆联动".to_string(),
            summary: "桌宠会把你的投资偏好、自选池和研究习惯写入长期记忆，后续分析会优先沿这些线索展开。".to_string(),
            bullets,
        });
    }

    let mut alerts = Vec::new();
    if config.watchlist.is_empty() && config.funds.is_empty() {
        alerts.push(ResearchBriefAlert {
            id: generate_id("research_alert"),
            severity: "watch".to_string(),
            title: "研究池为空".to_string(),
            summary: "你还没有配置股票/ETF 或基金观察池，建议先补充自选池。".to_string(),
        });
    }
    if config.habit_notes.is_empty() {
        alerts.push(ResearchBriefAlert {
            id: generate_id("research_alert"),
            severity: "info".to_string(),
            title: "建议补充投资习惯".to_string(),
            summary: "把你的择时习惯、风险偏好和止损原则写进“习惯备注”，桌宠后续分析会更贴合你。".to_string(),
        });
    }
    if !memory_hints.is_empty() {
        alerts.push(ResearchBriefAlert {
            id: generate_id("research_alert"),
            severity: "info".to_string(),
            title: "已加载长期记忆".to_string(),
            summary: format!("本次简报已载入 {} 条投研偏好/主题记忆。", memory_hints.len()),
        });
    }
    if let Some(quote) = fund_quotes
        .iter()
        .find(|item| item.change_percent.unwrap_or(0.0) >= 1.5)
    {
        alerts.push(ResearchBriefAlert {
            id: generate_id("research_alert"),
            severity: "watch".to_string(),
            title: "基金池出现较大上行".to_string(),
            summary: format!(
                "{} 当前估算涨幅约 {:.2}%，建议确认是风格驱动、主题驱动还是单日情绪放大。",
                quote.name,
                quote.change_percent.unwrap_or_default()
            ),
        });
    }
    if let Some(quote) = fund_quotes
        .iter()
        .find(|item| item.change_percent.unwrap_or(0.0) <= -1.5)
    {
        alerts.push(ResearchBriefAlert {
            id: generate_id("research_alert"),
            severity: "watch".to_string(),
            title: "基金池出现较大回撤".to_string(),
            summary: format!(
                "{} 当前估算跌幅约 {:.2}%，建议检查风格漂移、行业拖累或短线情绪回撤。",
                quote.name,
                quote.change_percent.unwrap_or_default()
            ),
        });
    }

    let alert_fingerprint = build_alert_fingerprint(&alerts);
    let has_new_day = runtime
        .research_status
        .last_daily_brief_day
        .as_deref()
        != Some(day_key.as_str());
    let startup_popup_due = runtime
        .research_status
        .last_startup_popup_day
        .as_deref()
        != Some(day_key.as_str());
    let has_new_alerts = !alert_fingerprint.is_empty()
        && runtime
            .research_status
            .last_alert_fingerprint
            .as_deref()
            != Some(alert_fingerprint.as_str());
    let has_updates = has_new_day || has_new_alerts;
    let update_summary = build_update_summary(has_new_day, has_new_alerts, alerts.len());
    let ai_analysis = build_ai_analysis(
        app,
        runtime,
        &config,
        &memory_hints,
        &sections,
        &alerts,
        &fund_quotes,
        &day_key,
    )
    .await;

    Ok(ResearchBriefSnapshot {
        generated_at,
        day_key,
        enabled: true,
        title: "今日投研简报".to_string(),
        summary: format!(
            "当前是本地研究模式，会优先围绕你的自选池、基金风格关注点、地缘主题和长期记忆来组织分析。"
        ),
        sections,
        alerts,
        fund_quotes,
        memory_hints,
        alert_fingerprint,
        has_updates,
        startup_popup_due,
        update_summary,
        analysis_status: ai_analysis.status,
        analysis_provider_label: ai_analysis.provider_label,
        analysis_result: ai_analysis.result,
        analysis_notice: ai_analysis.notice,
    })
}

pub fn acknowledge_brief(
    app: &AppHandle,
    runtime: &mut RuntimeState,
    day_key: &str,
    alert_fingerprint: &str,
    mark_startup_popup: bool,
) -> Result<(), String> {
    runtime.research_status.last_daily_brief_day = Some(day_key.trim().to_string());
    runtime.research_status.last_alert_fingerprint = if alert_fingerprint.trim().is_empty() {
        None
    } else {
        Some(alert_fingerprint.trim().to_string())
    };
    runtime.research_status.last_brief_generated_at = Some(now_millis());
    if mark_startup_popup {
        runtime.research_status.last_startup_popup_day = Some(day_key.trim().to_string());
    }
    save(app, runtime)
}

pub fn sync_research_memory(app: &AppHandle, config: &ResearchConfig) -> Result<(), String> {
    let config = normalize_config(config);
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let store = MemoryStore::new(&app_data);
    let now = now_millis();

    let semantic_payloads = [
        (
            "investment_watchlist",
            "投资关注标的",
            config.watchlist.join("、"),
            vec!["investment".to_string(), "watchlist".to_string()],
        ),
        (
            "investment_funds",
            "投资关注基金",
            config.funds.join("、"),
            vec!["investment".to_string(), "fund".to_string()],
        ),
        (
            "investment_themes",
            "投资关注主题",
            config.themes.join("、"),
            vec!["investment".to_string(), "theme".to_string()],
        ),
        (
            "investment_habit_notes",
            "投资习惯",
            config.habit_notes.clone(),
            vec!["investment".to_string(), "habit".to_string()],
        ),
        (
            "investment_decision_framework",
            "投资决策框架",
            config.decision_framework.clone(),
            vec!["investment".to_string(), "framework".to_string()],
        ),
    ];

    for (memory_key, topic, knowledge, tags) in semantic_payloads {
        if knowledge.trim().is_empty() {
            continue;
        }

        store.upsert_semantic_entry(SemanticEntry {
            id: format!("semantic-{memory_key}"),
            memory_key: memory_key.to_string(),
            topic: topic.to_string(),
            knowledge,
            source_type: "investment_profile".to_string(),
            confidence: 0.88,
            created_at: now,
            updated_at: now,
            tags,
            explicit: true,
            mention_count: 3,
            ttl: None,
            status: MemoryStatus::Active,
            conflict_group: None,
        })?;
    }

    let meta_entries = [
        (
            "research_mode_enabled",
            json!(config.enabled),
        ),
        (
            "research_startup_popup",
            json!(config.startup_popup),
        ),
        (
            "research_bubble_alerts",
            json!(config.bubble_alerts),
        ),
    ];

    for (preference, value) in meta_entries {
        store.upsert_meta_preference(MetaPreference {
            id: format!("meta-investment-{preference}"),
            category: "investment".to_string(),
            preference: preference.to_string(),
            value,
            confidence: 0.9,
            created_at: now,
            updated_at: now,
            explicit: true,
            ttl: None,
            status: MemoryStatus::Active,
            conflict_group: None,
        })?;
    }

    Ok(())
}

fn collect_investment_memory_hints(memory_service: &MemoryService) -> Vec<String> {
    let semantic = memory_service.load_semantic().unwrap_or_default();
    let meta = memory_service.load_meta().unwrap_or_default();
    let mut hints = Vec::new();

    for entry in semantic.entries {
        if entry.status != MemoryStatus::Active {
            continue;
        }
        if entry.tags.iter().any(|tag| tag == "investment") {
            hints.push(format!("{}：{}", entry.topic, entry.knowledge));
        }
    }

    for entry in meta.preferences {
        if entry.status != MemoryStatus::Active || entry.category != "investment" {
            continue;
        }
        hints.push(format!("偏好 {} = {}", entry.preference, entry.value));
    }

    hints.truncate(6);
    hints
}

fn normalize_list(values: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();

    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if normalized.iter().any(|existing| existing == trimmed) {
            continue;
        }
        normalized.push(trimmed.to_string());
    }

    normalized
}

fn build_analysis_actions(config: &ResearchConfig, memory_hints: &[String]) -> Vec<String> {
    let mut bullets = Vec::new();

    if let Some(symbol) = config.watchlist.first() {
        bullets.push(format!(
            "先复盘 {symbol} 的最近财报：收入、毛利率、现金流、负债与管理层指引。"
        ));
    }

    if let Some(fund) = config.funds.first() {
        bullets.push(format!(
            "对 {fund} 做风格拆解：仓位弹性、换手、回撤韧性、风格漂移。"
        ));
    }

    if let Some(theme) = config.themes.first() {
        bullets.push(format!(
            "围绕“{theme}”整理政策、利率、汇率、制裁与供应链变量，判断影响路径。"
        ));
    }

    if !config.habit_notes.is_empty() {
        bullets.push(format!("把你的习惯约束带入今天分析：{}", config.habit_notes));
    }

    if !memory_hints.is_empty() {
        bullets.push("优先用长期记忆里的投资偏好校正今天的研究顺序。".to_string());
    }

    if bullets.is_empty() {
        bullets.push("先补齐自选池、基金池和研究主题，再生成更具体的本地研究任务。".to_string());
    }

    bullets
}

fn build_fund_quote_bullets(fund_quotes: &[ResearchFundQuote]) -> Vec<String> {
    if fund_quotes.is_empty() {
        return vec!["当前还没有可用的基金涨幅快照，建议先填入 6 位基金代码。".to_string()];
    }

    fund_quotes
        .iter()
        .map(|quote| {
            let change = quote
                .change_percent
                .map(|value| format!("{value:+.2}%"))
                .unwrap_or_else(|| "暂无涨幅".to_string());
            let estimate_nav = quote
                .estimate_nav
                .map(|value| format!("{value:.4}"))
                .unwrap_or_else(|| "--".to_string());
            let estimate_time = quote
                .estimate_time
                .clone()
                .unwrap_or_else(|| "暂无估值时间".to_string());
            match quote.note.as_ref().filter(|value| !value.trim().is_empty()) {
                Some(note) => format!("{}（{}）：{}，{}。", quote.name, quote.code, change, note),
                None => format!(
                    "{}（{}）：当前估算涨跌 {}，估算净值 {}，时间 {}。",
                    quote.name, quote.code, change, estimate_nav, estimate_time
                ),
            }
        })
        .collect()
}

fn build_watchlist_bullets(config: &ResearchConfig) -> Vec<String> {
    let mut bullets = vec![
        "先看收入、毛利率、经营现金流、资本开支和净负债变化。".to_string(),
        "把一次性收益、减值、汇率影响、股权激励与主业经营分开看。".to_string(),
        "重点比较本季结果、全年指引、市场预期和管理层措辞的变化。".to_string(),
    ];

    if !config.watchlist.is_empty() {
        let focus = config.watchlist.iter().take(3).cloned().collect::<Vec<_>>().join(" / ");
        bullets.push(format!("今天优先把 {focus} 放进同一张表，对比盈利质量、现金流与估值支撑。"));
    }

    bullets
}

fn build_fund_bullets(config: &ResearchConfig) -> Vec<String> {
    let mut bullets = vec![
        "看偏大盘/中小盘、成长/价值、行业集中度和仓位弹性。".to_string(),
        "对比换手率、回撤韧性、风格漂移和基金经理风格稳定性。".to_string(),
        "区分 beta 驱动、赛道驱动和选股 alpha 驱动。".to_string(),
    ];

    if !config.funds.is_empty() {
        let focus = config.funds.iter().take(3).cloned().collect::<Vec<_>>().join(" / ");
        bullets.push(format!("把 {focus} 放在一起比较，重点看是否存在风格重叠、仓位拥挤和回撤特征差异。"));
    }

    bullets
}

fn build_theme_bullets(config: &ResearchConfig) -> Vec<String> {
    let mut bullets = config
        .themes
        .iter()
        .map(|theme| format!("围绕主题“{theme}”追踪供应链、利率、汇率、制裁和政策变化。"))
        .collect::<Vec<_>>();

    if bullets.is_empty() {
        bullets.push("先确定你今天最想追踪的宏观/行业主题，再补足地缘与政策影响路径。".to_string());
    }

    bullets
}

fn build_observation_checklist(config: &ResearchConfig, memory_hints: &[String]) -> Vec<String> {
    let mut bullets = Vec::new();

    if let Some(symbol) = config.watchlist.first() {
        bullets.push(format!("先确认 {symbol} 最近一季财报和管理层指引有没有新变化。"));
    }
    if let Some(fund) = config.funds.first() {
        bullets.push(format!("检查 {fund} 最近披露的重仓、风格漂移和回撤表现。"));
    }
    if let Some(theme) = config.themes.first() {
        bullets.push(format!("围绕“{theme}”整理今天最值得跟踪的政策、汇率和供应链变量。"));
    }
    if !config.habit_notes.is_empty() {
        bullets.push(format!("沿用你的研究习惯过滤结论：{}", config.habit_notes));
    }
    if !memory_hints.is_empty() {
        bullets.push("先回看长期记忆里的投研偏好，避免今天的判断偏离你长期使用的框架。".to_string());
    }

    if bullets.is_empty() {
        bullets.push("今天先补齐自选池、基金池或主题，再让桌宠组织更具体的研究动作。".to_string());
    }

    bullets
}

fn local_day_key(timestamp: u64) -> String {
    Local
        .timestamp_millis_opt(timestamp as i64)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d")
        .to_string()
}

fn build_alert_fingerprint(alerts: &[ResearchBriefAlert]) -> String {
    alerts
        .iter()
        .map(|alert| format!("{}:{}:{}", alert.severity, alert.title, alert.summary))
        .collect::<Vec<_>>()
        .join("|")
}

fn build_update_summary(has_new_day: bool, has_new_alerts: bool, alert_count: usize) -> Option<String> {
    match (has_new_day, has_new_alerts) {
        (true, true) => Some(format!("今日简报已刷新，且有 {alert_count} 条新的研究提醒。")),
        (true, false) => Some("已切换到新一天的投研简报。".to_string()),
        (false, true) => Some(format!("研究提醒有更新，当前共 {alert_count} 条。")),
        (false, false) => None,
    }
}

fn normalize_research_analysis_error(provider_kind: ProviderKind, raw: &str) -> String {
    let trimmed = raw.trim();
    let lowered = trimmed.to_lowercase();

    if lowered.contains("deactivated_workspace")
        || (lowered.contains("402 payment required") && lowered.contains("codex"))
    {
        return "Codex CLI 当前登录的 workspace 已失效，或当前账号没有可用的 Codex 调用权限，所以 AI 投研分析没有生成出来。请重新登录可用的 Codex 账号/工作区，或切换到其他可用 provider。".to_string();
    }

    if lowered.contains("payment required") {
        return format!(
            "{} 当前返回了付费/额度限制，AI 投研分析暂时不可用。请检查订阅、额度或工作区权限。",
            provider_kind.label()
        );
    }

    if lowered.contains("401") || lowered.contains("unauthorized") || lowered.contains("auth error")
    {
        return format!(
            "{} 当前认证失败，AI 投研分析暂时不可用。请检查登录状态、API Key 或 OAuth 凭据。",
            provider_kind.label()
        );
    }

    if lowered.contains("timeout") || lowered.contains("timed out") {
        return format!(
            "{} 响应超时，AI 投研分析这次没有生成完成。可以稍后手动刷新再试。",
            provider_kind.label()
        );
    }

    let first_line = trimmed
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(trimmed);

    format!("AI 投研分析生成失败：{first_line}")
}

fn normalize_research_analysis_result(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(reply) = value.get("reply").and_then(Value::as_str) {
            return reply.trim().to_string();
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return message.trim().to_string();
        }
        if let Some(content) = value
            .get("output_text")
            .or_else(|| value.get("content"))
            .and_then(Value::as_str)
        {
            return content.trim().to_string();
        }
    }

    trimmed.to_string()
}

async fn build_ai_analysis(
    app: &AppHandle,
    runtime: &RuntimeState,
    config: &ResearchConfig,
    memory_hints: &[String],
    sections: &[ResearchBriefSection],
    alerts: &[ResearchBriefAlert],
    fund_quotes: &[ResearchFundQuote],
    day_key: &str,
) -> ResearchAiAnalysis {
    if matches!(runtime.provider.kind, ProviderKind::Mock) {
        return ResearchAiAnalysis {
            status: "unavailable".to_string(),
            provider_label: None,
            result: None,
            notice: Some("当前使用的是 Mock provider，无法生成 AI 投研分析。".to_string()),
        };
    }

    if !runtime.provider.allow_network {
        return ResearchAiAnalysis {
            status: "unavailable".to_string(),
            provider_label: None,
            result: None,
            notice: Some("当前处于离线安全模式，AI 投研分析已跳过。".to_string()),
        };
    }

    let codex_runtime = resolve_for_app(app).ok();
    let codex_command = codex_runtime
        .as_ref()
        .and_then(|item| item.command.as_ref())
        .map(|path| path.to_string_lossy().to_string());
    let codex_home = codex_runtime
        .as_ref()
        .map(|item| item.home_root.to_string_lossy().to_string());
    let allowed_actions = policy::actions_for_level(runtime.permission_level);
    let prompt = build_research_analysis_prompt(
        config,
        memory_hints,
        sections,
        alerts,
        fund_quotes,
        day_key,
    );
    let history = vec![
        ChatMessage::new(
            "system",
            guardrails::compose_system_prompt(
                &runtime.provider,
                runtime.permission_level,
                &allowed_actions,
            ),
        ),
        ChatMessage::user(prompt),
    ];
    let mut research_thread_id = None;

    match provider::respond(
        &runtime.provider,
        runtime.api_key.clone(),
        runtime.oauth_access_token.clone(),
        codex_command,
        codex_home,
        &mut research_thread_id,
        runtime.permission_level,
        &allowed_actions,
        &history,
    )
    .await
    {
        Ok((reply, label)) => ResearchAiAnalysis {
            status: "ready".to_string(),
            provider_label: Some(label),
            result: Some(normalize_research_analysis_result(&reply)),
            notice: None,
        },
        Err(error) => ResearchAiAnalysis {
            status: "error".to_string(),
            provider_label: Some(runtime.provider.kind.label().to_string()),
            result: None,
            notice: Some(normalize_research_analysis_error(runtime.provider.kind, &error)),
        },
    }
}

fn build_research_analysis_prompt(
    config: &ResearchConfig,
    memory_hints: &[String],
    sections: &[ResearchBriefSection],
    alerts: &[ResearchBriefAlert],
    fund_quotes: &[ResearchFundQuote],
    day_key: &str,
) -> String {
    let watchlist = if config.watchlist.is_empty() {
        "无".to_string()
    } else {
        config.watchlist.join("、")
    };
    let funds = if config.funds.is_empty() {
        "无".to_string()
    } else {
        config.funds.join("、")
    };
    let themes = if config.themes.is_empty() {
        "无".to_string()
    } else {
        config.themes.join("、")
    };
    let memory_text = if memory_hints.is_empty() {
        "无".to_string()
    } else {
        memory_hints.join("\n- ")
    };
    let section_text = sections
        .iter()
        .map(|section| {
            format!(
                "### {}\n{}\n- {}",
                section.title,
                section.summary,
                section.bullets.join("\n- ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let alerts_text = if alerts.is_empty() {
        "无".to_string()
    } else {
        alerts
            .iter()
            .map(|alert| format!("[{}] {}：{}", alert.severity, alert.title, alert.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let fund_quote_text = if fund_quotes.is_empty() {
        "无".to_string()
    } else {
        fund_quotes
            .iter()
            .map(|quote| {
                let change = quote
                    .change_percent
                    .map(|value| format!("{value:+.2}%"))
                    .unwrap_or_else(|| "暂无涨幅".to_string());
                let estimate_time = quote
                    .estimate_time
                    .clone()
                    .unwrap_or_else(|| "暂无估值时间".to_string());
                let note = quote
                    .note
                    .as_ref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!("；备注：{value}"))
                    .unwrap_or_default();
                format!("{}（{}）：{}，时间 {}{}", quote.name, quote.code, change, estimate_time, note)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "你现在是桌宠内置的本地投研助手。请基于下面这些本地配置、长期记忆和简报骨架，直接给出一份中文研究分析。\n\
        重要约束：\n\
        1. 不要假装你看到了实时行情、实时新闻、最新财报或外部数据库；如果缺少实时数据，要明确写出“当前仅基于本地配置和长期记忆”。\n\
        2. 不要给出确定性的买卖指令、仓位建议或收益承诺。\n\
        3. 输出要面向个人研究使用，重点讲逻辑、比较、风险、失效条件和今天该先看什么。\n\
        4. 禁止反问用户、禁止要求用户补充资料、禁止把结果写成待办提问；信息不足时，也必须基于现有配置先给出 best-effort 成品分析。\n\
        5. 禁止输出 JSON、代码块或键值对包装；直接输出自然中文正文。\n\
        6. 每个部分都要给出明确判断，不要只写方法论；即使结论保守，也要把当前倾向、风险和下一步观察点说清楚。\n\
        7. 每个部分请拆成 2 到 3 个短段落，每段 1 到 3 句，不要把所有内容挤成一整段。\n\
        8. 请严格使用下面格式输出，并保持简洁有信息量：\n\
        【总判断】\n\
        【财报拆解重点】\n\
        【基金风格比较】\n\
        【主题/地缘影响链】\n\
        【决策框架】\n\
        【今日优先动作】\n\n\
        今日日期：{day_key}\n\
        股票/ETF：{watchlist}\n\
        基金：{funds}\n\
        主题：{themes}\n\
        投资习惯备注：{}\n\
        决策框架：{}\n\n\
        当前基金涨幅快照：\n\
        {fund_quote_text}\n\n\
        已加载长期记忆：\n\
        - {memory_text}\n\n\
        当前静态简报骨架：\n\
        {section_text}\n\n\
        当前研究提醒：\n\
        {alerts_text}",
        if config.habit_notes.trim().is_empty() {
            "无"
        } else {
            config.habit_notes.trim()
        },
        config.decision_framework.trim(),
        fund_quote_text = fund_quote_text,
    )
}

fn extract_fund_code(raw: &str) -> Option<String> {
    let digits = raw
        .chars()
        .filter(|item| item.is_ascii_digit())
        .collect::<String>();
    if digits.len() >= 6 {
        Some(digits[..6].to_string())
    } else {
        None
    }
}

fn parse_number(value: Option<&str>) -> Option<f64> {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .and_then(|item| item.parse::<f64>().ok())
}

fn parse_fund_estimate_text(raw: &str) -> Result<FundEstimatePayload, String> {
    let trimmed = raw.trim();
    let payload = trimmed
        .strip_prefix("jsonpgz(")
        .and_then(|item| item.strip_suffix(");"))
        .unwrap_or(trimmed);

    serde_json::from_str::<FundEstimatePayload>(payload)
        .map_err(|error| format!("解析基金估值数据失败: {error}"))
}

async fn fetch_fund_quotes(
    config: &ResearchConfig,
    allow_network: bool,
    stamp: u64,
) -> Vec<ResearchFundQuote> {
    if !allow_network {
        return config
            .funds
            .iter()
            .map(|item| ResearchFundQuote {
                code: extract_fund_code(item).unwrap_or_else(|| item.trim().to_string()),
                name: item.trim().to_string(),
                estimate_nav: None,
                previous_nav: None,
                change_percent: None,
                estimate_time: None,
                note: Some("当前已关闭网络访问，未获取实时基金涨幅。".to_string()),
            })
            .collect();
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("PenguinPal Assistant/0.2.0")
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return config
                .funds
                .iter()
                .map(|item| ResearchFundQuote {
                    code: extract_fund_code(item).unwrap_or_else(|| item.trim().to_string()),
                    name: item.trim().to_string(),
                    estimate_nav: None,
                    previous_nav: None,
                    change_percent: None,
                    estimate_time: None,
                    note: Some(format!("创建基金行情客户端失败: {error}")),
                })
                .collect();
        }
    };

    let mut quotes = Vec::new();
    for item in &config.funds {
        let label = item.trim();
        if label.is_empty() {
            continue;
        }

        let Some(code) = extract_fund_code(label) else {
            quotes.push(ResearchFundQuote {
                code: label.to_string(),
                name: label.to_string(),
                estimate_nav: None,
                previous_nav: None,
                change_percent: None,
                estimate_time: None,
                note: Some("未识别到 6 位基金代码，暂时无法拉取实时涨幅。".to_string()),
            });
            continue;
        };

        let url = format!("https://fundgz.1234567.com.cn/js/{code}.js?rt={stamp}");
        let quote = match client.get(&url).send().await {
            Ok(response) => match response.text().await {
                Ok(text) => match parse_fund_estimate_text(&text) {
                    Ok(payload) => ResearchFundQuote {
                        code: payload.fundcode.unwrap_or_else(|| code.clone()),
                        name: payload.name.unwrap_or_else(|| label.to_string()),
                        estimate_nav: parse_number(payload.gsz.as_deref()),
                        previous_nav: parse_number(payload.dwjz.as_deref()),
                        change_percent: parse_number(payload.gszzl.as_deref()),
                        estimate_time: payload
                            .gztime
                            .map(|value| value.trim().to_string())
                            .filter(|value| !value.is_empty()),
                        note: None,
                    },
                    Err(error) => ResearchFundQuote {
                        code: code.clone(),
                        name: label.to_string(),
                        estimate_nav: None,
                        previous_nav: None,
                        change_percent: None,
                        estimate_time: None,
                        note: Some(error),
                    },
                },
                Err(error) => ResearchFundQuote {
                    code: code.clone(),
                    name: label.to_string(),
                    estimate_nav: None,
                    previous_nav: None,
                    change_percent: None,
                    estimate_time: None,
                    note: Some(format!("读取基金涨幅响应失败: {error}")),
                },
            },
            Err(error) => ResearchFundQuote {
                code: code.clone(),
                name: label.to_string(),
                estimate_nav: None,
                previous_nav: None,
                change_percent: None,
                estimate_time: None,
                note: Some(format!("获取基金涨幅失败: {error}")),
            },
        };
        quotes.push(quote);
    }

    quotes
}

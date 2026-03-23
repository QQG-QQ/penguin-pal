use chrono::{Local, TimeZone};
use serde::Serialize;
use serde_json::json;
use tauri::AppHandle;

use crate::{
    app_state::{now_millis, save, ResearchConfig, RuntimeState},
    memory::{generate_id, MemoryService, MemoryStatus, MemoryStore, MetaPreference, SemanticEntry},
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
pub struct ResearchBriefSnapshot {
    pub generated_at: u64,
    pub day_key: String,
    pub enabled: bool,
    pub title: String,
    pub summary: String,
    pub sections: Vec<ResearchBriefSection>,
    pub alerts: Vec<ResearchBriefAlert>,
    pub memory_hints: Vec<String>,
    pub alert_fingerprint: String,
    pub has_updates: bool,
    pub update_summary: Option<String>,
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

pub fn build_brief(app: &AppHandle, runtime: &RuntimeState) -> Result<ResearchBriefSnapshot, String> {
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
            memory_hints: Vec::new(),
            alert_fingerprint: String::new(),
            has_updates: false,
            update_summary: None,
        });
    }

    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = MemoryService::new(&app_data);
    let memory_hints = collect_investment_memory_hints(&memory_service);

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

    let alert_fingerprint = build_alert_fingerprint(&alerts);
    let has_new_day = runtime
        .research_status
        .last_daily_brief_day
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
        memory_hints,
        alert_fingerprint,
        has_updates,
        update_summary,
    })
}

pub fn acknowledge_brief(
    app: &AppHandle,
    runtime: &mut RuntimeState,
    day_key: &str,
    alert_fingerprint: &str,
) -> Result<(), String> {
    runtime.research_status.last_daily_brief_day = Some(day_key.trim().to_string());
    runtime.research_status.last_alert_fingerprint = if alert_fingerprint.trim().is_empty() {
        None
    } else {
        Some(alert_fingerprint.trim().to_string())
    };
    runtime.research_status.last_brief_generated_at = Some(now_millis());
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

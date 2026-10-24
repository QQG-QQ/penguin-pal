//! Shared companion state. Optional on old documents; never contains connection profiles.
use crate::companion_sync::{scrub, valid_id};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const EMOTIONS: [&str; 7] = [
    "happy",
    "calm",
    "sweet",
    "heartache",
    "worry",
    "sad",
    "jealous",
];
fn emotions() -> BTreeMap<String, f64> {
    EMOTIONS.iter().map(|s| (s.to_string(), 0.0)).collect()
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub interval_minutes: u32,
    pub cooldown_minutes: u32,
    pub daily_limit: u32,
    pub probability: f64,
    pub miss_bonus: f64,
    pub bonus_cap: f64,
    pub quiet_start: u32,
    pub quiet_end: u32,
    pub dry_run: bool,
    pub motive: String,
    pub recent_minutes: u32,
    pub distant_minutes: u32,
    pub memory_half_days: f64,
    pub max_recall: u32,
    pub recall_chars: u32,
    pub facts_max: u32,
    pub daily_memories: u32,
    pub evidence: f64,
    pub freshness_weight: f64,
    pub importance_weight: f64,
    pub emotion_weight: f64,
    pub user_name: String,
    pub pet_name: String,
    pub pronoun: String,
    pub mood_enabled: bool,
    pub inject_mode: String,
    pub confidence: f64,
    pub baseline: f64,
    pub mood_floor: f64,
    pub mood_half_hours: f64,
    pub emotion_half_minutes: f64,
    pub romance: bool,
    pub model: String,
    pub animations: bool,
    pub badge: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            interval_minutes: 30,
            cooldown_minutes: 45,
            daily_limit: 4,
            probability: 60.0,
            miss_bonus: 10.0,
            bonus_cap: 40.0,
            quiet_start: 22,
            quiet_end: 8,
            dry_run: false,
            motive: "围绕共同记忆或近期事情，自然说一两句话，只开一个话题。".into(),
            recent_minutes: 20,
            distant_minutes: 360,
            memory_half_days: 30.0,
            max_recall: 8,
            recall_chars: 1600,
            facts_max: 3,
            daily_memories: 30,
            evidence: 0.25,
            freshness_weight: 0.3,
            importance_weight: 0.2,
            emotion_weight: 0.1,
            user_name: "你".into(),
            pet_name: "企鹅".into(),
            pronoun: "ta".into(),
            mood_enabled: true,
            inject_mode: "event".into(),
            confidence: 60.0,
            baseline: 70.0,
            mood_floor: 0.0,
            mood_half_hours: 12.0,
            emotion_half_minutes: 90.0,
            romance: false,
            model: String::new(),
            animations: true,
            badge: true,
        }
    }
}
fn in_range(n: f64, lo: f64, hi: f64) -> bool {
    n.is_finite() && (lo..=hi).contains(&n)
}
impl Config {
    pub fn valid(&self) -> bool {
        (1..=1440).contains(&self.interval_minutes)
            && self.cooldown_minutes <= 1440
            && (1..=100).contains(&self.daily_limit)
            && self.quiet_start < 24
            && self.quiet_end < 24
            && self.recent_minutes <= 1440
            && self.distant_minutes <= 10080
            && in_range(self.probability, 0.0, 100.0)
            && in_range(self.miss_bonus, 0.0, 100.0)
            && in_range(self.bonus_cap, 0.0, 100.0)
            && in_range(self.memory_half_days, 1.0, 3650.0)
            && (1..=24).contains(&self.max_recall)
            && (200..=12000).contains(&self.recall_chars)
            && self.facts_max <= 24
            && (1..=1000).contains(&self.daily_memories)
            && in_range(self.evidence, 0.0, 1.0)
            && [
                self.freshness_weight,
                self.importance_weight,
                self.emotion_weight,
            ]
            .iter()
            .all(|n| in_range(*n, 0.0, 1.0))
            && [
                self.user_name.as_str(),
                self.pet_name.as_str(),
                self.pronoun.as_str(),
            ]
            .iter()
            .all(|s| !s.trim().is_empty() && s.chars().count() <= 40)
            && self.motive.chars().count() <= 400
            && self.model.chars().count() <= 100
            && ["off", "state", "event"].contains(&self.inject_mode.as_str())
            && in_range(self.confidence, 0.0, 100.0)
            && in_range(self.baseline, 30.0, 100.0)
            && in_range(self.mood_floor, 0.0, self.baseline)
            && in_range(self.mood_half_hours, 1.0, 168.0)
            && in_range(self.emotion_half_minutes, 1.0, 1440.0)
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Version {
    pub text: String,
    pub at: u64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Meta {
    pub kind: String,
    pub key: String,
    pub importance: f64,
    pub emotion: f64,
    pub tags: Vec<String>,
    pub reinforced_at: u64,
    pub strength: f64,
    pub created_at: u64,
    pub historical: bool,
    pub versions: Vec<Version>,
}
impl Default for Meta {
    fn default() -> Self {
        Self {
            kind: "recent".into(),
            key: String::new(),
            importance: 0.5,
            emotion: 0.0,
            tags: vec![],
            reinforced_at: 0,
            strength: 1.0,
            created_at: 0,
            historical: false,
            versions: vec![],
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub at: u64,
    pub source: String,
    pub reason: String,
    pub confidence: f64,
    pub delta: f64,
    pub dims: BTreeMap<String, f64>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    pub at: u64,
    pub reason: String,
    pub chance: f64,
    pub roll: f64,
    pub next_at: u64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Brain {
    pub config: Config,
    pub memories: BTreeMap<String, Meta>,
    pub emotions: BTreeMap<String, f64>,
    pub emotion_at: u64,
    pub event: Option<Event>,
    pub consumed: String,
    pub consumed_ids: Vec<String>,
    pub injection: String,
    pub injected_at: u64,
    pub timeline: Vec<Event>,
    pub traits: BTreeMap<String, f64>,
    pub traits_at: u64,
    pub decisions: Vec<Decision>,
    pub next_check_at: u64,
    pub misses: u32,
    pub manual: bool,
}
impl Default for Brain {
    fn default() -> Self {
        Self {
            config: Config::default(),
            memories: BTreeMap::new(),
            emotions: emotions(),
            emotion_at: 0,
            event: None,
            consumed: String::new(),
            consumed_ids: vec![],
            injection: String::new(),
            injected_at: 0,
            timeline: vec![],
            traits: emotions(),
            traits_at: 0,
            decisions: vec![],
            next_check_at: 0,
            misses: 0,
            manual: false,
        }
    }
}
impl Brain {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
    pub fn valid(&self) -> bool {
        self.config.valid()
            && self.memories.len() <= 2000
            && self.timeline.len() <= 200
            && self.decisions.len() <= 60
            && self.emotions.len() == 7
            && EMOTIONS.iter().all(|k| {
                self.emotions
                    .get(*k)
                    .is_some_and(|n| in_range(*n, 0.0, 100.0))
            })
            && self.misses <= 1000
            && self.injection.chars().count() <= 600
            && (self.consumed.is_empty() || valid_id(&self.consumed))
            && self.consumed_ids.len() <= 200
            && self.consumed_ids.iter().all(|s| valid_id(s))
            && self.memories.iter().all(|(id, m)| {
                valid_id(id)
                    && ["preference", "agreement", "recent"].contains(&m.kind.as_str())
                    && m.key.chars().count() <= 60
                    && in_range(m.importance, 0.0, 1.0)
                    && in_range(m.emotion, 0.0, 1.0)
                    && in_range(m.strength, 0.0, 1.0)
                    && m.tags.len() <= 12
                    && m.tags.iter().all(|t| t.chars().count() <= 40)
                    && m.versions.len() <= 8
                    && m.versions.iter().all(|v| v.text.chars().count() <= 2000)
            })
            && self.timeline.iter().chain(self.event.iter()).all(|e| {
                valid_id(&e.id)
                    && e.reason.chars().count() <= 400
                    && ["user", "speak-up", "manual", "decay"].contains(&e.source.as_str())
                    && in_range(e.confidence, 0.0, 100.0)
                    && in_range(e.delta, -10.0, 10.0)
                    && e.dims.len() <= 7
                    && e.dims
                        .iter()
                        .all(|(k, n)| EMOTIONS.contains(&k.as_str()) && in_range(*n, 0.0, 100.0))
            })
            && self.decisions.iter().all(|d| {
                d.reason.chars().count() <= 200
                    && in_range(d.chance, 0.0, 100.0)
                    && in_range(d.roll, 0.0, 100.0)
            })
    }
    pub fn sanitize(&mut self) {
        self.config.user_name = scrub(&self.config.user_name, 40);
        self.config.pet_name = scrub(&self.config.pet_name, 40);
        self.config.pronoun = scrub(&self.config.pronoun, 40);
        self.config.motive = scrub(&self.config.motive, 400);
        self.config.model = scrub(&self.config.model, 100);
        for m in self.memories.values_mut() {
            m.key = scrub(&m.key, 60);
            for t in &mut m.tags {
                *t = scrub(t, 40)
            }
            for v in &mut m.versions {
                v.text = scrub(&v.text, 2000)
            }
        }
        for e in self.timeline.iter_mut().chain(self.event.iter_mut()) {
            e.reason = scrub(&e.reason, 400)
        }
        self.injection = scrub(&self.injection, 600);
        for d in &mut self.decisions {
            d.reason = scrub(&d.reason, 200)
        }
    }
    pub fn advance(&mut self, at: u64) {
        if self.emotion_at == 0 {
            return;
        }
        if self.emotion_at > 0 {
            let minutes = at.saturating_sub(self.emotion_at) as f64 / 60000.0;
            let factor = 0.5_f64.powf(minutes / self.config.emotion_half_minutes);
            for (name, value) in self.emotions.iter_mut() {
                let base = self.traits.get(name).copied().unwrap_or(0.0);
                *value = base + (*value - base) * factor;
            }
        }
        self.emotion_at = at.max(self.emotion_at);
    }
}
pub fn load(doc: &Value) -> Result<Brain, String> {
    let mut brain: Brain = serde_json::from_value(doc.get("brain").cloned().unwrap_or_else(|| json!({})))
        .map_err(|_| String::from("陪伴扩展数据无效。"))?;
    brain.sanitize();
    let traits = derive_traits(doc, &brain);
    brain.traits = traits.clone();
    if brain.emotions.values().all(|value| *value <= 0.01) {
        brain.emotions = traits;
    }
    brain.advance(chrono::Utc::now().timestamp_millis() as u64);
    Ok(brain)
}

/// 由性格模板、记忆体量和历史情绪沉淀出七维基线。
pub fn derive_traits(doc: &Value, brain: &Brain) -> BTreeMap<String, f64> {
    let personality = doc["pet"]["personality"].as_str().unwrap_or("gentle");
    let template: [(&str, f64); 7] = match personality {
        "cheerful" | "happy" => [("happy", 42.0), ("calm", 22.0), ("sweet", 14.0), ("heartache", 5.0), ("worry", 8.0), ("sad", 4.0), ("jealous", 5.0)],
        "tsundere" => [("happy", 20.0), ("calm", 18.0), ("sweet", 10.0), ("heartache", 8.0), ("worry", 6.0), ("sad", 4.0), ("jealous", 18.0)],
        _ => [("happy", 30.0), ("calm", 36.0), ("sweet", 12.0), ("heartache", 8.0), ("worry", 10.0), ("sad", 6.0), ("jealous", 5.0)],
    };
    let memories = doc["memories"].as_array().map(|list| list.len()).unwrap_or(0) as f64;
    let volume = (memories / 60.0).min(10.0);
    let mut history: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    for event in brain.timeline.iter().rev().take(60) {
        for (key, value) in event.dims.iter() {
            let entry = history.entry(match key.as_str() {
                "happy" => "happy",
                "calm" => "calm",
                "sweet" => "sweet",
                "heartache" => "heartache",
                "worry" => "worry",
                "sad" => "sad",
                "jealous" => "jealous",
                _ => continue,
            }).or_insert((0.0, 0.0));
            entry.0 += *value;
            entry.1 += 1.0;
        }
    }
    let mut traits = emotions();
    for (name, base) in template {
        let average = history.get(name).map(|(sum, count)| sum / count.max(1.0)).unwrap_or(base);
        let mut value = base * 0.7 + average * 0.3;
        if matches!(name, "calm" | "happy") {
            value += volume * 0.6;
        }
        if !brain.config.romance && matches!(name, "sweet" | "jealous") {
            value = 0.0;
        }
        traits.insert(name.to_string(), (value.clamp(0.0, 100.0) * 10.0).round() / 10.0);
    }
    traits
}
fn save(doc: &mut Value, mut brain: Brain) -> Result<(), String> {
    brain.sanitize();
    if !brain.valid() {
        return Err("陪伴参数或记录超过限制。".into());
    }
    doc["brain"] = serde_json::to_value(brain).unwrap();
    Ok(())
}

pub fn remember(
    doc: &mut Value,
    id: &str,
    memory: &Value,
    old: Option<&Value>,
    source_at: u64,
    at: u64,
    historical: bool,
) -> Result<bool, String> {
    let mut brain = load(doc)?;
    if memory.get("tags").is_some_and(|v| {
        v.as_array().is_none_or(|a| {
            a.len() > 12
                || a.iter()
                    .any(|t| t.as_str().is_none_or(|s| s.chars().count() > 40))
        })
    }) || ["importance", "emotion"].iter().any(|k| {
        memory
            .get(*k)
            .is_some_and(|v| v.as_f64().is_none_or(|n| !in_range(n, 0.0, 1.0)))
    }) {
        return Err("记忆属性无效。".into());
    }
    if !historical
        && !brain.memories.contains_key(id)
        && brain
            .memories
            .values()
            .filter(|m| !m.historical && m.created_at / 86400000 == at / 86400000)
            .count()
            >= brain.config.daily_memories as usize
    {
        return Ok(false);
    }
    let meta = brain.memories.entry(id.into()).or_default();
    if let Some(old) = old {
        let body = old["text"].as_str().unwrap_or("");
        if body.lines().next().is_some_and(|s| {
            let label = match memory["kind"].as_str().unwrap_or("") {
                "preference" => "喜好",
                "agreement" => "约定",
                _ => "近况",
            };
            s == format!("【{label}】{}", memory["text"].as_str().unwrap_or(""))
        }) {
            let anchor = if meta.reinforced_at == 0 {
                old["at"].as_u64().unwrap_or(source_at)
            } else {
                meta.reinforced_at
            };
            let age = source_at.saturating_sub(anchor) as f64 / 86400000.0;
            let current = meta.strength
                * 0.5_f64
                    .powf(age / (brain.config.memory_half_days * (1.0 + 2.0 * meta.importance)));
            meta.strength = current + 0.2 * (1.0 - current);
        } else {
            meta.versions.push(Version {
                text: body.into(),
                at: old["at"].as_u64().unwrap_or(0),
            });
            if meta.versions.len() > 8 {
                meta.versions.remove(0);
            }
            meta.strength = 1.0;
        }
    }
    meta.kind = memory["kind"].as_str().unwrap_or("recent").into();
    meta.key = scrub(memory["key"].as_str().unwrap_or(""), 60);
    meta.importance = memory["importance"].as_f64().unwrap_or(0.5);
    meta.emotion = memory["emotion"].as_f64().unwrap_or(0.0);
    meta.tags = memory["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|s| scrub(s, 40))
                .collect()
        })
        .unwrap_or_default();
    meta.reinforced_at = source_at;
    if meta.created_at == 0 {
        meta.created_at = at;
        meta.historical = historical;
    }
    save(doc, brain)?;
    Ok(true)
}
pub fn mood(doc: &mut Value, data: &Value, id: &str, at: u64, source: &str) -> Result<(), String> {
    let mut brain = load(doc)?;
    if !brain.config.mood_enabled {
        return Ok(());
    }
    brain.advance(at);
    brain.emotion_at = at;
    let delta = data["delta"]
        .as_f64()
        .filter(|n| in_range(*n, -3.0, 3.0))
        .ok_or("心情变化无效。")?;
    let confidence = match data.get("confidence") {
        Some(v) => v.as_f64().ok_or("心情可信度无效。")?,
        None => 70.0,
    };
    if !in_range(confidence, 0.0, 100.0) {
        return Err("心情可信度无效。".into());
    }
    let mut changed = delta != 0.0;
    if let Some(dims) = data.get("dims") {
        let dims = dims.as_object().ok_or("情绪维度无效。")?;
        if dims
            .iter()
            .any(|(k, v)| !EMOTIONS.contains(&k.as_str()) || v.as_f64().is_none())
        {
            return Err("情绪维度无效。".into());
        }
    }
    for name in EMOTIONS {
        let n = data["dims"][name].as_f64().unwrap_or(0.0);
        if !in_range(n, -6.0, 6.0) {
            return Err("情绪变化无效。".into());
        }
        if !brain.config.romance && ["sweet", "jealous"].contains(&name) {
            continue;
        }
        changed |= n != 0.0;
        let v = brain.emotions.get_mut(name).unwrap();
        *v = (*v + n).clamp(0.0, 100.0);
    }
    if !changed {
        return Ok(());
    }
    let reason = scrub(data["reason"].as_str().unwrap_or(""), 160);
    let quote = scrub(data["quote"].as_str().unwrap_or(""), 160);
    let reason = if quote.is_empty() {
        reason
    } else {
        format!("{reason}；依据：「{quote}」")
    };
    let value = (doc["pet"]["mood"].as_f64().unwrap_or(brain.config.baseline) + delta)
        .clamp(brain.config.mood_floor, 100.0);
    doc["pet"]["mood"] = json!(value);
    doc["pet"]["moodReason"] = json!(reason);
    doc["pet"]["moodAt"] = json!(at);
    let event = Event {
        id: id.into(),
        at,
        source: source.into(),
        reason: reason.clone(),
        confidence,
        delta,
        dims: brain.emotions.clone(),
    };
    brain.event = Some(event.clone());
    brain.timeline.push(event);
    if brain.timeline.len() > 200 {
        brain.timeline.remove(0);
    }
    let list = doc["moodHistory"].as_array_mut().ok_or("缺少心情记录。")?;
    list.push(json!({"id":id,"value":value,"reason":reason,"at":at}));
    if list.len() > 200 {
        list.remove(0);
    }
    save(doc, brain)
}
pub fn apply(doc: &mut Value, data: &Value, id: &str, at: u64) -> Result<(), String> {
    let mut brain = load(doc)?;
    match data["type"].as_str().unwrap_or("") {
        "settings" => {
            let mut cfg = serde_json::to_value(&brain.config).unwrap();
            let patch = data["config"].as_object().ok_or("设置无效。")?;
            for (k, v) in patch {
                if cfg.get(k).is_none() {
                    return Err("未知的陪伴设置。".into());
                }
                cfg[k] = v.clone();
            }
            brain.config = serde_json::from_value(cfg).map_err(|_| "陪伴参数格式无效。")?;
            brain.next_check_at = 0;
            if !brain.config.romance {
                for k in ["sweet", "jealous"] {
                    brain.emotions.insert(k.into(), 0.0);
                }
            }
        }
        "consume" => {
            if brain.timeline.iter().any(|e| data["eventId"] == e.id)
                && !brain.consumed_ids.iter().any(|id| data["eventId"] == *id)
            {
                brain.consumed = data["eventId"].as_str().unwrap_or("").into();
                brain.consumed_ids.push(brain.consumed.clone());
                if brain.consumed_ids.len() > 200 {
                    brain.consumed_ids.remove(0);
                }
                brain.injection = scrub(data["text"].as_str().unwrap_or(""), 600);
                brain.injected_at = at;
            }
        }
        "mergeMeta" => {
            let key = data["id"]
                .as_str()
                .filter(|s| valid_id(s))
                .ok_or("记忆标识无效。")?;
            if !brain.memories.contains_key(key)
                && doc["memories"].as_array().is_some_and(|a| {
                    a.iter().any(|m| {
                        m["id"] == key && m["text"] == data["expected"] && m["deleted"] != true
                    })
                })
            {
                let meta: Meta =
                    serde_json::from_value(data["meta"].clone()).map_err(|_| "记忆元数据无效。")?;
                brain.memories.insert(key.into(), meta);
            }
        }
        "decision" => {
            if at < brain.next_check_at {
                return Ok(());
            }
            let chance = data["chance"].as_f64().unwrap_or(0.0);
            let roll = data["roll"].as_f64().unwrap_or(0.0);
            brain.next_check_at = at + brain.config.interval_minutes as u64 * 60000;
            brain.misses = if roll < chance {
                0
            } else {
                (brain.misses + 1).min(1000)
            };
            brain.decisions.push(Decision {
                at,
                reason: scrub(data["reason"].as_str().unwrap_or(""), 200),
                chance,
                roll,
                next_at: brain.next_check_at,
            });
            if brain.decisions.len() > 60 {
                brain.decisions.remove(0);
            }
        }
        "log" => {
            brain.decisions.push(Decision {
                at,
                reason: scrub(data["reason"].as_str().unwrap_or(""), 200),
                ..Default::default()
            });
            if brain.decisions.len() > 60 {
                brain.decisions.remove(0);
            }
        }
        "resetMood" => {
            doc["pet"]["mood"] = json!(brain.config.baseline);
            doc["pet"]["moodReason"] = json!("已重置到平稳心情");
            doc["pet"]["moodAt"] = json!(at);
            doc["moodHistory"] = json!([]);
            brain.emotions = emotions();
            brain.emotion_at = at;
            brain.event = None;
            brain.timeline.clear();
            brain.consumed.clear();
            brain.consumed_ids.clear();
            brain.injection.clear();
        }
        "editMemory" | "undoMemory" => {
            let key = data["id"]
                .as_str()
                .filter(|s| valid_id(s))
                .ok_or("记忆标识无效。")?;
            let item = doc["memories"]
                .as_array_mut()
                .ok_or("缺少记忆。")?
                .iter_mut()
                .find(|m| m["id"] == key && m["deleted"] != true)
                .ok_or("记忆已遗忘或不存在。")?;
            if data["expected"] != item["text"] {
                return Err("记忆已在另一台设备更新，请刷新后重试。".into());
            }
            let meta = brain.memories.entry(key.into()).or_default();
            if data["type"] == "undoMemory" {
                let previous = meta.versions.pop().ok_or("没有可恢复的旧版本。")?;
                item["text"] = json!(previous.text);
                item["at"] = json!(previous.at);
            } else {
                let mut update = serde_json::to_value(&*meta).unwrap();
                for name in ["importance", "emotion", "tags", "key", "kind"] {
                    if let Some(v) = data.get(name) {
                        update[name] = v.clone();
                    }
                }
                *meta = serde_json::from_value(update).map_err(|_| "记忆属性无效。")?;
            }
        }
        _ => return Err("不支持的陪伴扩展操作。".into()),
    }
    let _ = id;
    save(doc, brain)
}

#[cfg(test)]
mod trait_tests {
    use serde_json::json;

    #[test]
    fn traits_form_baseline() {
        let doc = json!({
            "pet": {"personality": "gentle"},
            "memories": [{"id": "a", "text": "喜欢咖啡"}, {"id": "b", "text": "喜欢猫"}],
            "brain": {}
        });
        let brain = super::load(&doc).unwrap();
        assert!(brain.emotions.values().any(|value| *value > 5.0), "{:?}", brain.emotions);
        assert!(brain.traits["calm"] >= 30.0, "{:?}", brain.traits);
        assert_eq!(brain.traits["sweet"], 0.0);
    }
}
use crate::companion_sync::{digest, scrub, valid_id};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Social {
    pub enabled: bool,
    pub proactive: bool,
    pub analyzed: Vec<String>,
    pub last_outreach_at: u64,
    pub last_response_at: u64,
    pub unanswered: u32,
    pub last_topic: String,
    pub reservation: String,
    pub reserved_until: u64,
    pub day: u64,
    pub sent_today: u32,
}
impl Default for Social {
    fn default() -> Self {
        Self {
            enabled: true,
            proactive: false,
            analyzed: vec![],
            last_outreach_at: 0,
            last_response_at: 0,
            unanswered: 0,
            last_topic: String::new(),
            reservation: String::new(),
            reserved_until: 0,
            day: 0,
            sent_today: 0,
        }
    }
}
impl Social {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
    pub fn valid(&self) -> bool {
        self.analyzed.len() <= 500
            && self.analyzed.iter().all(|x| valid_id(x))
            && self.last_topic.chars().count() <= 100
            && (self.reservation.is_empty() || valid_id(&self.reservation))
            && self.unanswered <= 1000
            && self.sent_today <= 1000
    }
    pub fn can_send(&self, at: u64) -> bool {
        self.proactive
            && (self.day != at / 86400000 || self.sent_today < 4)
            && (self.last_outreach_at == 0
                || at.saturating_sub(self.last_outreach_at) >= 45 * 60000)
            && self.reserved_until <= at
    }
}
pub fn pet_session(_pet_id: &str) -> String {
    // The document/namespace owns the pet identity. A local pet joining an existing
    // shared identity must keep its bubble history in the same visible timeline.
    digest(b"pet-bubble:v1")
}
fn text(data: &Value, key: &str, limit: usize) -> Result<String, String> {
    let raw = data[key].as_str().ok_or("陪伴记录缺少正文。")?;
    if raw.chars().count() > limit {
        return Err("陪伴记录超过长度限制。".into());
    }
    Ok(scrub(raw, limit))
}
fn message(doc: &mut Value, id: &str, role: &str, body: &str, at: u64) {
    let session = pet_session(doc["petId"].as_str().unwrap_or_default());
    let list = doc["conversations"].as_array_mut().unwrap();
    if !list.iter().any(|m| m["id"] == id) {
        list.push(json!({"id":id,"session":session,"role":role,"text":body,"at":at}));
    }
    list.sort_by_key(|m| {
        (
            m["at"].as_u64().unwrap_or(0),
            m["id"].as_str().unwrap_or("").to_owned(),
        )
    });
    if list.len() > 500 {
        list.drain(..list.len() - 500);
    }
}
pub fn apply(doc: &mut Value, data: &Value, op_id: &str, at: u64) -> Result<(), String> {
    let mut social: Social =
        serde_json::from_value(doc.get("social").cloned().unwrap_or_else(|| json!({})))
            .map_err(|_| "陪伴状态无效。")?;
    match data["type"].as_str().ok_or("陪伴操作无效。")? {
        "settings" => {
            social.enabled = data["enabled"].as_bool().ok_or("开关无效。")?;
            social.proactive = data["proactive"].as_bool().ok_or("开关无效。")?;
            if !social.proactive {
                social.reservation.clear();
                social.reserved_until = 0;
            }
        }
        "user" => {
            let body = text(data, "text", 3000)?;
            if body.trim().is_empty() {
                return Err("请输入聊天内容。".into());
            }
            message(doc, op_id, "user", &body, at);
            social.last_response_at = at;
            social.unanswered = 0;
            // A user response invalidates a not-yet-delivered opener.
            social.reservation.clear();
            social.reserved_until = 0;
        }
        "reply" => {
            let body = text(data, "text", 1200)?;
            if body.trim().is_empty() {
                return Err("回复为空。".into());
            }
            message(doc, op_id, "assistant", &body, at);
        }
        "analysis" | "history" => {
            let historical = data["type"] == "history";
            let source = text(data, "sourceId", 80)?;
            if !valid_id(&source) {
                return Err("消息来源无效。".into());
            }
            if social.analyzed.contains(&source) {
                return Ok(());
            }
            let source_message = if historical {
                let message = &data["source"];
                if message["id"] != source
                    || message["role"] != "user"
                    || !message["session"].as_str().is_some_and(valid_id)
                    || message["text"]
                        .as_str()
                        .is_none_or(|s| s.chars().count() > 6000)
                    || message["at"].as_u64().is_none()
                {
                    return Err("历史来源无效。".into());
                }
                Some(message.clone())
            } else {
                doc["conversations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["id"] == source && m["role"] == "user")
                    .cloned()
            };
            let Some(source_message) = source_message else {
                return Ok(());
            };
            let original = source_message["text"]
                .as_str()
                .ok_or("来源原文无效。")?
                .to_owned();
            let source_at = source_message["at"].as_u64().unwrap_or(at).min(at);
            let session = source_message["session"].as_str().unwrap_or("");
            let memories = data["memories"]
                .as_array()
                .filter(|m| m.len() <= 3)
                .ok_or("学习结果无效。")?;
            for memory in memories {
                let kind = memory["kind"].as_str().ok_or("记忆分类无效。")?;
                let label = match kind {
                    "preference" => "喜好",
                    "agreement" => "约定",
                    "recent" => "近况",
                    _ => return Err("记忆分类无效。".into()),
                };
                let key = text(memory, "key", 60)?;
                let body = text(memory, "text", 400)?;
                let quote = text(memory, "quote", 160)?;
                if key.trim().is_empty()
                    || body.trim().is_empty()
                    || quote.chars().count() < 2
                    || !original.contains(&quote)
                {
                    return Err("记忆没有用户原话依据。".into());
                }
                let id = digest(format!("learned:{kind}:{key}").as_bytes());
                let list = doc["memories"].as_array_mut().unwrap();
                if list.iter().any(|m| {
                    m["id"] == id
                        && (m["deleted"] == true || m["at"].as_u64().unwrap_or(0) > source_at)
                }) {
                    continue;
                }
                list.retain(|m| m["id"] != id);
                list.push(json!({"id":id,"text":format!("【{label}】{body}\n依据：「{quote}」\n来源：{source} / {session} / {source_at}"),"at":source_at,"deleted":false}));
            }
            let delta = data["delta"]
                .as_i64()
                .filter(|v| (-3..=3).contains(v))
                .ok_or("心情变化无效。")?;
            if delta != 0 && !historical {
                let reason = text(data, "reason", 160)?;
                let quote = text(data, "quote", 160)?;
                if reason.trim().is_empty()
                    || quote.chars().count() < 2
                    || !original.contains(&quote)
                {
                    return Err("心情没有用户原话依据。".into());
                }
                let mood =
                    (doc["pet"]["mood"].as_f64().unwrap_or(70.0) + delta as f64).clamp(0.0, 100.0);
                let reason = format!("{reason}；依据：「{quote}」");
                doc["pet"]["mood"] = json!(mood);
                doc["pet"]["moodReason"] = json!(reason);
                doc["pet"]["moodAt"] = json!(at);
                let list = doc["moodHistory"].as_array_mut().unwrap();
                list.push(json!({"id":op_id,"value":mood,"reason":reason,"at":at}));
                if list.len() > 200 {
                    list.remove(0);
                }
            }
            social.analyzed.push(source);
            if social.analyzed.len() > 500 {
                social.analyzed.remove(0);
            }
        }
        "reserve" => {
            let token = text(data, "token", 80)?;
            if !valid_id(&token) {
                return Err("搭话标识无效。".into());
            }
            if social.can_send(at) {
                social.reservation = token;
                social.reserved_until = at + 180000;
            }
        }
        "release" => {
            if data["token"] == social.reservation {
                social.reservation.clear();
                social.reserved_until = 0;
            }
        }
        "commit" => {
            if !social.proactive
                || data["token"] != social.reservation
                || social.reservation.is_empty()
                || social.reserved_until <= at
            {
                return Ok(());
            }
            let body = text(data, "text", 1200)?;
            let topic = text(data, "topic", 100)?;
            if body.trim().is_empty() || topic.trim().is_empty() {
                return Err("主动话题为空。".into());
            }
            if !social.last_topic.is_empty() && social.last_topic == topic {
                social.reservation.clear();
                social.reserved_until = 0;
                doc["social"] = serde_json::to_value(social).unwrap();
                return Ok(());
            }
            message(doc, op_id, "assistant", &body, at);
            if social.day != at / 86400000 {
                social.day = at / 86400000;
                social.sent_today = 0;
            }
            social.sent_today += 1;
            social.unanswered = social.unanswered.saturating_add(1).min(1000);
            social.last_outreach_at = at;
            social.last_topic = topic;
            social.reservation.clear();
            social.reserved_until = 0;
        }
        _ => return Err("不支持的陪伴操作。".into()),
    }
    if !social.valid() {
        return Err("陪伴状态超过限制。".into());
    }
    doc["social"] = serde_json::to_value(social).unwrap();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document() -> Value {
        json!({"petId":"pet","pet":{"mood":70},"conversations":[{"id":"user-1","role":"user","text":"我喜欢清淡的小鱼，谢谢你陪我","at":1}],"memories":[],"moodHistory":[]})
    }
    #[test]
    fn learning_requires_evidence_is_idempotent_and_keeps_tombstones() {
        let mut doc = document();
        let data = json!({"type":"analysis","sourceId":"user-1","memories":[{"kind":"preference","key":"饮食","text":"喜欢清淡的小鱼","quote":"我喜欢清淡的小鱼"}],"delta":2,"reason":"受到感谢","quote":"谢谢你陪我"});
        apply(&mut doc, &data, "op", 100).unwrap();
        assert_eq!(doc["pet"]["mood"].as_f64(), Some(72.0));
        let saved = doc.clone();
        apply(&mut doc, &data, "retry", 101).unwrap();
        assert_eq!(doc, saved);
        let mut invalid = document();
        let mut bad = data.clone();
        bad["quote"] = json!("凭空捏造");
        assert!(apply(&mut invalid, &bad, "bad", 1).is_err());
        let mut deleted = document();
        deleted["memories"] =
            json!([{"id":digest("learned:preference:饮食".as_bytes()),"deleted":true}]);
        apply(&mut deleted, &data, "new", 100).unwrap();
        assert_eq!(deleted["memories"].as_array().unwrap().len(), 1);
        assert_eq!(deleted["memories"][0]["deleted"], true);
    }
    #[test]
    fn simultaneous_reservations_fixed_cooldown_and_response_cancel() {
        let mut doc = document();
        apply(
            &mut doc,
            &json!({"type":"settings","enabled":true,"proactive":true}),
            "s",
            1,
        )
        .unwrap();
        apply(&mut doc, &json!({"type":"reserve","token":"A"}), "a", 1000).unwrap();
        apply(&mut doc, &json!({"type":"reserve","token":"B"}), "b", 1000).unwrap();
        assert_eq!(doc["social"]["reservation"], "A");
        apply(
            &mut doc,
            &json!({"type":"commit","token":"B","text":"wrong","topic":"wrong"}),
            "wrong",
            1001,
        )
        .unwrap();
        assert_eq!(doc["conversations"].as_array().unwrap().len(), 1);
        apply(
            &mut doc,
            &json!({"type":"commit","token":"A","text":"小鱼喜欢什么口味？","topic":"小鱼"}),
            "reply",
            1001,
        )
        .unwrap();
        let social: Social = serde_json::from_value(doc["social"].clone()).unwrap();
        assert!(!social.can_send(1001 + 45 * 60000 - 1));
        assert!(social.can_send(1001 + 45 * 60000));
        apply(
            &mut doc,
            &json!({"type":"reserve","token":"C"}),
            "c",
            1001 + 45 * 60000,
        )
        .unwrap();
        apply(
            &mut doc,
            &json!({"type":"user","text":"我回来啦"}),
            "user-2",
            1002 + 45 * 60000,
        )
        .unwrap();
        assert_eq!(doc["social"]["unanswered"], 0);
        assert_eq!(doc["social"]["reservation"], "");
    }
    #[test]
    fn unanswered_never_pauses_outreach_but_daily_limit_still_applies() {
        let mut social = Social {
            proactive: true,
            unanswered: 1000,
            last_outreach_at: 1000,
            day: 0,
            sent_today: 3,
            ..Default::default()
        };
        assert!(!social.can_send(1000 + 45 * 60000 - 1));
        assert!(social.can_send(1000 + 45 * 60000));
        social.sent_today = 4;
        assert!(!social.can_send(1000 + 45 * 60000));
        assert!(social.can_send(86400000));
    }
    #[test]
    fn historical_evidence_survives_eviction_uses_source_time_and_never_replays_mood() {
        let mut doc = document();
        doc["conversations"] = json!([]);
        let original = json!({"id":"old-1","session":"thread","role":"user","text":"喜欢清淡小鱼，谢谢你陪我","at":100});
        let mut data = json!({"type":"history","sourceId":"old-1","source":original,"memories":[{"kind":"preference","key":"饮食","text":"喜欢清淡小鱼","quote":"喜欢清淡小鱼"}],"delta":3,"reason":"受到感谢","quote":"谢谢你陪我"});
        apply(&mut doc, &data, "history-1", 100000).unwrap();
        assert_eq!(doc["memories"][0]["at"], 100);
        assert!(doc["memories"][0]["text"]
            .as_str()
            .unwrap()
            .contains("来源：old-1 / thread / 100"));
        assert_eq!(doc["pet"]["mood"], 70);
        assert!(doc["moodHistory"].as_array().unwrap().is_empty());
        data["sourceId"] = json!("older");
        data["source"]["id"] = json!("older");
        data["source"]["at"] = json!(50);
        data["memories"][0]["text"] = json!("不该覆盖新喜好");
        apply(&mut doc, &data, "older-op", 100001).unwrap();
        assert!(!doc["memories"][0]["text"]
            .as_str()
            .unwrap()
            .contains("不该覆盖"));
        data["sourceId"] = json!("bad");
        data["source"]["id"] = json!("bad");
        data["source"]["role"] = json!("assistant");
        assert!(apply(&mut doc, &data, "invalid", 100002).is_err());
        let mut doc = document();
        doc["conversations"] = json!([]);
        data["source"]["role"] = json!("user");
        data["memories"][0]["quote"] = json!("不在原文中");
        assert!(apply(&mut doc, &data, "fake-evidence", 100003).is_err());
    }
}

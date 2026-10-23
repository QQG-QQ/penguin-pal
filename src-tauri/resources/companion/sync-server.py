"""Private, data-only sync endpoint invoked over SSH; no network listener."""
import hashlib
import json
import math
import os
import re
import sqlite3
import sys
import time
import uuid
from pathlib import Path

LIMIT = 8 * 1024 * 1024
PERSONALITIES = {"gentle", "playful", "quiet", "warm"}


def ident(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,80}", value):
        raise ValueError("invalid identifier")
    return value


def clean(value, maximum=6000):
    if not isinstance(value, str) or len(value) > maximum:
        raise ValueError("invalid text size")
    value = re.sub(r"(?s)-----BEGIN [^-]*PRIVATE KEY-----.*?(?:-----END [^-]*PRIVATE KEY-----|$)", "[凭据已移除]", value)
    value = re.sub(r"\bsk-(?:proj-)?[A-Za-z0-9_-]{16,}", "[凭据已移除]", value)
    value = re.sub(r"(?i)(bearer\s+)[A-Za-z0-9._~+/-]{12,}", r"\1[已移除]", value)
    value = re.sub(r"(?i)(?:password|passwd|api[_-]?key|access[_-]?token|密码)[\"']?\s*[:=：]\s*[\"']?[^\s,，;；\"']+", "[凭据已移除]", value)
    return re.sub(r"[A-Za-z][A-Za-z0-9+.-]*://[^/\s:@]+:[^@\s/]+@", "[凭据已移除]", value)


def number(value, low=0, high=100):
    if isinstance(value, bool) or not isinstance(value, (float, int)) or not math.isfinite(value) or not low <= value <= high:
        raise ValueError("invalid number")
    return value


def initial(seed, now):
    if isinstance(seed, dict) and seed.get("schema") == 1:
        document = sanitize_document(validate(json.loads(json.dumps(seed))))
        advance(document, now)
        document["revision"] = 0
        return document
    return {"schema": 1, "petId": str(uuid.uuid4()), "revision": 0, "updatedAt": now,
            "pet": {"energy": number(seed.get("energy", 80)), "hunger": number(seed.get("hunger", 20)),
                    "affection": number(seed.get("affection", 65)), "personality": seed.get("personality", "gentle"),
                    "mood": 70, "moodReason": "", "moodAt": now},
            "memories": [], "conversations": [], "moodHistory": [], "lastFeedAt": 0, "lastPatAt": 0}


def validate(doc):
    if not isinstance(doc, dict) or set(doc) - {"social", "brain"} != {"schema", "petId", "revision", "updatedAt", "pet", "memories", "conversations", "moodHistory", "lastFeedAt", "lastPatAt"} or doc["schema"] != 1:
        raise ValueError("unsupported data format")
    ident(doc["petId"])
    if "brain" in doc:
        validate_brain(doc["brain"])
    for key in ["revision", "updatedAt", "lastFeedAt", "lastPatAt"]:
        number(doc[key], 0, 1e16)
    pet = doc["pet"]
    if not isinstance(pet, dict) or set(pet) != {"energy", "hunger", "affection", "personality", "mood", "moodReason", "moodAt"}:
        raise ValueError("invalid pet")
    for key in ["energy", "hunger", "affection", "mood"]:
        number(pet[key])
    if pet["personality"] not in PERSONALITIES:
        raise ValueError("invalid personality")
    clean(pet["moodReason"], 400)
    number(pet["moodAt"], 0, 1e16)
    if "social" in doc:
        social = doc["social"]
        if not isinstance(social, dict) or set(social) != set(social_default()):
            raise ValueError("invalid social state")
        if not isinstance(social["enabled"], bool) or not isinstance(social["proactive"], bool) or not isinstance(social["analyzed"], list) or len(social["analyzed"]) > 500:
            raise ValueError("invalid social policy")
        for item in social["analyzed"]:
            ident(item)
        for key in ["lastOutreachAt", "lastResponseAt", "reservedUntil", "day"]:
            number(social[key], 0, 1e16)
        for key in ["unanswered", "sentToday"]:
            number(social[key], 0, 1000)
        clean(social["lastTopic"], 100)
        if social["reservation"]:
            ident(social["reservation"])
    specs = {
        "memories": (2000, {"id", "text", "at", "deleted"}),
        "conversations": (500, {"id", "session", "role", "text", "at"}),
        "moodHistory": (200, {"id", "value", "reason", "at"})}
    for name, (limit, fields) in specs.items():
        if not isinstance(doc[name], list) or len(doc[name]) > limit:
            raise ValueError("collection exceeds limit")
        seen = set()
        for item in doc[name]:
            if not isinstance(item, dict) or set(item) != fields:
                raise ValueError("invalid record")
            key = ident(item["id"])
            if key in seen:
                raise ValueError("duplicate record")
            seen.add(key); number(item["at"], 0, 1e16)
            if name == "memories":
                if not isinstance(item["deleted"], bool):
                    raise ValueError("invalid tombstone")
                clean(item["text"], 2000)
            elif name == "conversations":
                ident(item["session"]); clean(item["text"])
                if item["role"] not in {"user", "assistant"}:
                    raise ValueError("invalid role")
            else:
                number(item["value"]); clean(item["reason"], 400)
    return doc


def sanitize_document(doc):
    if "brain" in doc:
        sanitize_brain(doc["brain"])
    doc["pet"]["moodReason"] = clean(doc["pet"]["moodReason"], 400)
    for item in doc["memories"]:
        item["text"] = "" if item["deleted"] else clean(item["text"], 2000)
    for item in doc["conversations"]:
        item["text"] = clean(item["text"])
    for item in doc["moodHistory"]:
        item["reason"] = clean(item["reason"], 400)
    return doc


def advance(doc, now):
    hours = min(24, max(0, now - doc["updatedAt"]) / 3600000)
    pet = doc["pet"]
    pet["energy"] = min(100, pet["energy"] + hours * 2)
    pet["hunger"] = min(100, pet["hunger"] + hours * 2)
    pet["affection"] = max(0, pet["affection"] - hours / 4)
    cfg = doc.get("brain", brain_default())["config"]
    pet["mood"] = min(100, max(cfg["moodFloor"], cfg["baseline"] + (pet["mood"] - cfg["baseline"]) * math.pow(0.5, hours / cfg["moodHalfHours"])))
    if "brain" in doc:
        advance_brain(doc["brain"], now)
    doc["updatedAt"] = max(now, doc["updatedAt"])


def apply(doc, op, now):
    ident(op["id"])
    if set(op) != {"id", "kind", "data"} or not isinstance(op["data"], dict):
        raise ValueError("invalid operation")
    data, pet = op["data"], doc["pet"]
    if op["kind"] == "social":
        apply_social(doc, data, op["id"], now)
    elif op["kind"] == "brain":
        apply_brain(doc, data, op["id"], now)
    elif op["kind"] == "care":
        action = data.get("action")
        if action not in {"pat", "poke", "tail", "feed", "rest"}:
            raise ValueError("invalid care")
        if action == "feed" and pet["hunger"] >= 8 and now - doc["lastFeedAt"] >= 5000:
            pet["hunger"] = max(0, pet["hunger"] - 18); pet["energy"] = min(100, pet["energy"] + 5)
            pet["affection"] = min(100, pet["affection"] + 2); doc["lastFeedAt"] = now
        elif action == "pat" and now - doc["lastPatAt"] >= 3000:
            pet["affection"] = min(100, pet["affection"] + 1); doc["lastPatAt"] = now
        elif action == "rest":
            pet["energy"] = min(100, pet["energy"] + 3)
    elif op["kind"] == "personality":
        if data.get("value") not in PERSONALITIES:
            raise ValueError("invalid personality")
        pet["personality"] = data["value"]
    elif op["kind"] == "memory":
        key = ident(data.get("id")); text = clean(data.get("text", ""), 2000)
        if not isinstance(data.get("deleted"), bool):
            raise ValueError("invalid tombstone")
        old = next((x for x in doc["memories"] if x["id"] == key), None)
        if old and old["deleted"]:
            return
        at = min(now, number(data.get("at", now), 0, 1e16))
        if old and not data["deleted"] and old["at"] > at:
            return
        if old:
            doc["memories"].remove(old)
        if len(doc["memories"]) >= 2000:
            raise ValueError("memory limit reached")
        doc["memories"].append({"id": key, "text": "" if data["deleted"] else text, "deleted": data["deleted"], "at": at})
        if data["deleted"] and "brain" in doc:
            doc["brain"]["memories"].pop(key, None)
    elif op["kind"] == "conversation":
        item = {"id": ident(data.get("id")), "session": ident(data.get("session")), "role": data.get("role"),
                "text": clean(data.get("text")), "at": min(now, number(data.get("at"), 0, 1e16))}
        if item["role"] not in {"user", "assistant"}:
            raise ValueError("invalid role")
        if not any(x["id"] == item["id"] for x in doc["conversations"]):
            doc["conversations"].append(item)
            doc["conversations"] = sorted(doc["conversations"], key=lambda x: (x["at"], x["id"]))[-500:]
    elif op["kind"] == "mood":
        pet["mood"] = min(100, max(0, pet["mood"] + number(data.get("delta"), -10, 10)))
        pet["moodReason"] = clean(data.get("reason", ""), 400); pet["moodAt"] = now
        doc["moodHistory"].append({"id": op["id"], "value": pet["mood"], "reason": pet["moodReason"], "at": now})
        doc["moodHistory"] = doc["moodHistory"][-200:]
    elif op["kind"] == "restore":
        imported = sanitize_document(validate(data.get("document")))
        for item in imported["memories"]:
            apply(doc, {"id": op["id"], "kind": "memory", "data": {k: item[k] for k in ["id", "text", "at", "deleted"]}}, now)
        for item in imported["conversations"]:
            apply(doc, {"id": op["id"], "kind": "conversation", "data": item}, now)
        # Explicit restore keeps the existing shared identity; collections merge.
        doc["pet"] = imported["pet"].copy()
        doc["moodHistory"] = imported["moodHistory"][-200:]
        previous = doc.get("brain", brain_default())
        brain = json.loads(json.dumps(imported.get("brain", brain_default())))
        for field in ["nextCheckAt", "misses", "decisions"]:
            brain[field] = previous[field]
        brain["manual"] = False
        brain["memories"] = {k: v for k, v in brain["memories"].items() if any(m["id"] == k and not m["deleted"] for m in doc["memories"])}
        doc["brain"] = brain
    else:
        raise ValueError("unknown operation")


def social_default():
    return {"enabled": True, "proactive": False, "analyzed": [], "lastOutreachAt": 0, "lastResponseAt": 0,
            "unanswered": 0, "lastTopic": "", "reservation": "", "reservedUntil": 0, "day": 0, "sentToday": 0}


def can_send(social, now, config=None):
    config = config or brain_default()["config"]
    return (social["proactive"] and
            (social["day"] != now // 86400000 or social["sentToday"] < config["dailyLimit"]) and
            (social["lastOutreachAt"] == 0 or now - social["lastOutreachAt"] >= config["cooldownMinutes"] * 60000) and
            social["reservedUntil"] <= now)


def social_message(doc, key, role, text, now):
    session = hashlib.sha256(b"pet-bubble:v1").hexdigest()
    if not any(x["id"] == key for x in doc["conversations"]):
        doc["conversations"].append({"id": key, "session": session, "role": role, "text": text, "at": now})
        doc["conversations"] = sorted(doc["conversations"], key=lambda x: (x["at"], x["id"]))[-500:]


def apply_social(doc, data, key, now):
    social = doc.setdefault("social", social_default())
    kind = data.get("type")
    if kind == "settings":
        if not isinstance(data.get("enabled"), bool) or not isinstance(data.get("proactive"), bool):
            raise ValueError("invalid settings")
        social["enabled"], social["proactive"] = data["enabled"], data["proactive"]
        if not social["proactive"]:
            social["reservation"], social["reservedUntil"] = "", 0
    elif kind == "user":
        text = clean(data.get("text"), 3000)
        if not text.strip():
            raise ValueError("empty message")
        social_message(doc, key, "user", text, now)
        social["lastResponseAt"], social["unanswered"] = now, 0
        social["reservation"], social["reservedUntil"] = "", 0
    elif kind == "reply":
        text = clean(data.get("text"), 1200)
        if not text.strip():
            raise ValueError("empty reply")
        social_message(doc, key, "assistant", text, now)
    elif kind in {"analysis", "history"}:
        historical = kind == "history"
        source_id = ident(data.get("sourceId"))
        if source_id in social["analyzed"]:
            return
        if historical:
            source_message = data.get("source", {})
            if not isinstance(source_message, dict) or source_message.get("id") != source_id or source_message.get("role") != "user":
                raise ValueError("invalid historical source")
            ident(source_message.get("session")); clean(source_message.get("text"))
            number(source_message.get("at"), 0, 1e16)
        else:
            source_message = next((x for x in doc["conversations"] if x["id"] == source_id and x["role"] == "user"), None)
        if source_message is None:
            return
        source = source_message["text"]
        source_at = min(now, source_message.get("at", now))
        session = source_message.get("session", "")
        memories = data.get("memories")
        if not isinstance(memories, list) or len(memories) > 3:
            raise ValueError("invalid memories")
        for memory in memories:
            labels = {"preference": "喜好", "agreement": "约定", "recent": "近况"}
            kind = memory.get("kind")
            if kind not in labels:
                raise ValueError("invalid memory kind")
            attribute, text, quote = clean(memory.get("key"), 60), clean(memory.get("text"), 400), clean(memory.get("quote"), 160)
            if not attribute.strip() or not text.strip() or len(quote) < 2 or quote not in source:
                raise ValueError("missing memory evidence")
            memory_id = hashlib.sha256(("learned:" + kind + ":" + attribute + (":" + source_id if kind == "recent" else "")).encode()).hexdigest()
            memory_id = next((k for k, m in sorted(doc.get("brain", {}).get("memories", {}).items()) if m["kind"] == kind and m["key"] == attribute and (kind != "recent" or any(item["id"] == k and item["text"].split("\n")[0] == "【"+labels[kind]+"】"+text for item in doc["memories"]))), memory_id)
            old = next((m for m in doc["memories"] if m["id"] == memory_id), None)
            latest = doc.get("brain", {}).get("memories", {}).get(memory_id, {}).get("reinforcedAt", 0)
            if old and (old["deleted"] or max(old["at"], latest) > source_at):
                continue
            if not remember_brain(doc, memory_id, memory, old, source_at, now, historical):
                continue
            repeated = old and old["text"].split("\n")[0] == "【" + labels[kind] + "】" + text
            text = "【" + labels[kind] + "】" + text + "\n依据：「" + quote + "」\n来源：" + source_id + " / " + session + " / " + str(source_at)
            if old:
                doc["memories"].remove(old)
            if len(doc["memories"]) >= 2000:
                raise ValueError("memory limit reached")
            doc["memories"].append(old if repeated else {"id": memory_id, "text": text, "deleted": False, "at": source_at})
        delta = number(data.get("delta"), -3, 3)
        if not isinstance(delta, int):
            raise ValueError("invalid mood delta")
        if (delta or any(data.get("dims", {}).values())) and not historical:
            reason, quote = clean(data.get("reason"), 160), clean(data.get("quote"), 160)
            if not reason.strip() or len(quote) < 2 or quote not in source:
                raise ValueError("missing mood evidence")
            mood_brain(doc, data, key, now, "user")
        social["analyzed"] = (social["analyzed"] + [source_id])[-500:]
    elif kind == "reserve":
        token = ident(data.get("token"))
        brain = doc.setdefault("brain", brain_default())
        manual = data.get("manual", False) is True
        if (manual and social["reservedUntil"] <= now) or can_send(social, now, brain["config"]):
            social["reservation"], social["reservedUntil"] = token, now + 180000
            brain["manual"] = manual
    elif kind == "release":
        if data.get("token") == social["reservation"]:
            social["reservation"], social["reservedUntil"] = "", 0
    elif kind == "commit":
        if (not social["proactive"] and not doc.get("brain", {}).get("manual", False)) or not social["reservation"] or data.get("token") != social["reservation"] or social["reservedUntil"] <= now:
            return
        text, topic = clean(data.get("text"), 1200), clean(data.get("topic"), 100)
        if not text.strip() or not topic.strip():
            raise ValueError("empty opener")
        if topic != social["lastTopic"]:
            social_message(doc, key, "assistant", text, now)
            if social["day"] != now // 86400000:
                social["day"], social["sentToday"] = now // 86400000, 0
            social["sentToday"] += 1
            social["unanswered"] = min(1000, social["unanswered"] + 1)
            social["lastOutreachAt"], social["lastTopic"] = now, topic
            if "selfMood" in data:
                feeling = data["selfMood"].copy(); feeling["quote"] = ""
                mood_brain(doc, feeling, key, now, "speak-up")
        social["reservation"], social["reservedUntil"] = "", 0
    else:
        raise ValueError("invalid social operation")


EMOTIONS = ["happy", "calm", "sweet", "heartache", "worry", "sad", "jealous"]


def brain_default():
    return {"config": {"intervalMinutes": 30, "cooldownMinutes": 45, "dailyLimit": 4,
        "probability": 60.0, "missBonus": 10.0, "bonusCap": 40.0, "quietStart": 22, "quietEnd": 8,
        "dryRun": False, "motive": "围绕共同记忆或近期事情，自然说一两句话，只开一个话题。",
        "recentMinutes": 20, "distantMinutes": 360, "memoryHalfDays": 30.0, "maxRecall": 8,
        "recallChars": 1600, "factsMax": 3, "dailyMemories": 30, "evidence": 0.25,
        "freshnessWeight": 0.3, "importanceWeight": 0.2, "emotionWeight": 0.1,
        "userName": "你", "petName": "企鹅", "pronoun": "ta", "moodEnabled": True,
        "injectMode": "event", "confidence": 60.0, "baseline": 70.0, "moodFloor": 0.0,
        "moodHalfHours": 12.0, "emotionHalfMinutes": 90.0, "romance": False, "model": "",
        "animations": True, "badge": True}, "memories": {}, "emotions": {k: 0.0 for k in EMOTIONS},
        "emotionAt": 0, "event": None, "consumed": "", "consumedIds": [], "injection": "", "injectedAt": 0,
        "timeline": [], "decisions": [], "nextCheckAt": 0, "misses": 0, "manual": False}


def meta_default():
    return dict(kind="recent", key="", importance=0.5, emotion=0.0, tags=[], reinforcedAt=0,
                strength=1.0, createdAt=0, historical=False, versions=[])


def validate_brain(brain):
    default = brain_default()
    if not isinstance(brain, dict) or set(brain) != set(default):
        raise ValueError("invalid companion extension")
    cfg = brain["config"]
    if not isinstance(cfg, dict) or set(cfg) != set(default["config"]):
        raise ValueError("invalid companion configuration")
    integer_ranges = {"intervalMinutes": (1, 1440), "cooldownMinutes": (0, 1440), "dailyLimit": (1, 100),
        "quietStart": (0, 23), "quietEnd": (0, 23), "recentMinutes": (0, 1440), "distantMinutes": (0, 10080),
        "maxRecall": (1, 24), "recallChars": (200, 12000), "factsMax": (0, 24), "dailyMemories": (1, 1000)}
    for key, limits in integer_ranges.items():
        number(cfg[key], *limits)
        if not isinstance(cfg[key], int):
            raise ValueError("expected integer configuration")
    for key in ["probability", "missBonus", "bonusCap", "confidence"]:
        number(cfg[key])
    for key in ["evidence", "freshnessWeight", "importanceWeight", "emotionWeight"]:
        number(cfg[key], 0, 1)
    for key, limits in {"memoryHalfDays": (1, 3650), "baseline": (30, 100), "moodFloor": (0, cfg["baseline"]),
                        "moodHalfHours": (1, 168), "emotionHalfMinutes": (1, 1440)}.items():
        number(cfg[key], *limits)
    for key in ["dryRun", "moodEnabled", "romance", "animations", "badge"]:
        if not isinstance(cfg[key], bool):
            raise ValueError("invalid switch")
    for key in ["userName", "petName", "pronoun"]:
        if not clean(cfg[key], 40).strip():
            raise ValueError("empty companion name")
    clean(cfg["motive"], 400); clean(cfg["model"], 100)
    if cfg["injectMode"] not in {"off", "state", "event"}:
        raise ValueError("invalid injection mode")
    if set(brain["emotions"]) != set(EMOTIONS):
        raise ValueError("invalid emotion dimensions")
    for value in brain["emotions"].values():
        number(value)
    for key in ["emotionAt", "injectedAt", "nextCheckAt"]:
        number(brain[key], 0, 1e16)
    number(brain["misses"], 0, 1000); clean(brain["injection"], 600)
    if brain["consumed"]:
        ident(brain["consumed"])
    if not isinstance(brain["consumedIds"], list) or len(brain["consumedIds"]) > 200:
        raise ValueError("invalid consumed events")
    for event_id in brain["consumedIds"]:
        ident(event_id)
    if not isinstance(brain["manual"], bool) or not isinstance(brain["memories"], dict) or len(brain["memories"]) > 2000:
        raise ValueError("invalid brain state")
    for key, meta in brain["memories"].items():
        ident(key)
        if set(meta) != set(meta_default()) or meta["kind"] not in {"preference", "agreement", "recent"}:
            raise ValueError("invalid memory metadata")
        clean(meta["key"], 60)
        for field in ["importance", "emotion", "strength"]:
            number(meta[field], 0, 1)
        for field in ["reinforcedAt", "createdAt"]:
            number(meta[field], 0, 1e16)
        if not isinstance(meta["historical"], bool) or not isinstance(meta["tags"], list) or len(meta["tags"]) > 12 or not isinstance(meta["versions"], list) or len(meta["versions"]) > 8:
            raise ValueError("invalid memory metadata collections")
        for tag in meta["tags"]:
            clean(tag, 40)
        for version in meta["versions"]:
            if set(version) != {"text", "at"}:
                raise ValueError("invalid memory version")
            clean(version["text"], 2000); number(version["at"], 0, 1e16)
    if not isinstance(brain["timeline"], list) or len(brain["timeline"]) > 200 or not isinstance(brain["decisions"], list) or len(brain["decisions"]) > 60:
        raise ValueError("invalid companion timeline")
    for event in brain["timeline"] + ([brain["event"]] if brain["event"] is not None else []):
        if set(event) != {"id", "at", "source", "reason", "confidence", "delta", "dims"} or event["source"] not in {"user", "speak-up", "manual", "decay"}:
            raise ValueError("invalid mood event")
        ident(event["id"]); number(event["at"], 0, 1e16); clean(event["reason"], 400)
        number(event["confidence"]); number(event["delta"], -10, 10)
        if not isinstance(event["dims"], dict) or set(event["dims"]) - set(EMOTIONS):
            raise ValueError("invalid mood dimensions")
        for value in event["dims"].values():
            number(value)
    for item in brain["decisions"]:
        if set(item) != {"at", "reason", "chance", "roll", "nextAt"}:
            raise ValueError("invalid decision record")
        number(item["at"], 0, 1e16); number(item["nextAt"], 0, 1e16)
        number(item["chance"]); number(item["roll"]); clean(item["reason"], 200)
    return brain


def sanitize_brain(brain):
    for key in ["userName", "petName", "pronoun", "motive", "model"]:
        brain["config"][key] = clean(brain["config"][key], 400)
    for meta in brain["memories"].values():
        meta["key"] = clean(meta["key"], 60)
        meta["tags"] = [clean(t, 40) for t in meta["tags"]]
        for version in meta["versions"]:
            version["text"] = clean(version["text"], 2000)
    for event in brain["timeline"] + ([brain["event"]] if brain["event"] else []):
        event["reason"] = clean(event["reason"], 400)
    brain["injection"] = clean(brain["injection"], 600)
    for item in brain["decisions"]:
        item["reason"] = clean(item["reason"], 200)


def advance_brain(brain, now):
    if not brain["emotionAt"]:
        return
    if brain["emotionAt"]:
        minutes = max(0, now - brain["emotionAt"]) / 60000
        brain["emotions"] = {k: v * 0.5 ** (minutes / brain["config"]["emotionHalfMinutes"]) for k, v in brain["emotions"].items()}
    brain["emotionAt"] = max(now, brain["emotionAt"])


def remember_brain(doc, key, memory, old, source_at, now, historical):
    brain = doc.setdefault("brain", brain_default())
    if not historical and key not in brain["memories"] and sum(not m["historical"] and m["createdAt"] // 86400000 == now // 86400000 for m in brain["memories"].values()) >= brain["config"]["dailyMemories"]:
        return False
    meta = brain["memories"].setdefault(key, meta_default())
    if old:
        label = {"preference": "喜好", "agreement": "约定", "recent": "近况"}[memory["kind"]]
        if old["text"].split("\n")[0] == "【" + label + "】" + memory["text"]:
            current = meta["strength"] * 0.5 ** (max(0, source_at-(meta["reinforcedAt"] or old["at"]))/86400000/(brain["config"]["memoryHalfDays"]*(1+2*meta["importance"])))
            meta["strength"] = current + 0.2*(1-current)
        else:
            meta["versions"] = (meta["versions"] + [dict(text=old["text"], at=old["at"])])[-8:]
            meta["strength"] = 1.0
    meta.update(kind=memory["kind"], key=clean(memory["key"], 60), importance=memory.get("importance", 0.5),
                emotion=memory.get("emotion", 0.0), tags=memory.get("tags", []), reinforcedAt=source_at)
    if meta["createdAt"] == 0:
        meta.update(createdAt=now, historical=historical)
    validate_brain(brain); sanitize_brain(brain)
    return True


def mood_brain(doc, data, key, now, source):
    brain = doc.setdefault("brain", brain_default())
    cfg = brain["config"]
    if not cfg["moodEnabled"]:
        return
    advance_brain(brain, now)
    brain["emotionAt"] = now
    delta, confidence = number(data["delta"], -3, 3), number(data.get("confidence", 70))
    if not isinstance(data.get("dims", {}), dict) or set(data.get("dims", {})) - set(EMOTIONS):
        raise ValueError("invalid emotion dimensions")
    changed = delta != 0
    for name in EMOTIONS:
        value = number(data.get("dims", {}).get(name, 0), -6, 6)
        if not cfg["romance"] and name in {"sweet", "jealous"}:
            continue
        changed = changed or value != 0
        brain["emotions"][name] = min(100, max(0, brain["emotions"][name] + value))
    if not changed:
        return
    reason, quote = clean(data.get("reason", ""), 160), clean(data.get("quote", ""), 160)
    reason += "；依据：「" + quote + "」" if quote else ""
    pet = doc["pet"]
    pet.update(mood=min(100, max(cfg["moodFloor"], pet["mood"] + delta)), moodReason=reason, moodAt=now)
    event = dict(id=key, at=now, source=source, reason=reason, confidence=confidence, delta=delta, dims=brain["emotions"].copy())
    brain["event"] = event
    brain["timeline"] = (brain["timeline"] + [event])[-200:]
    doc["moodHistory"] = (doc["moodHistory"] + [dict(id=key, value=pet["mood"], reason=reason, at=now)])[-200:]
    validate_brain(brain)


def apply_brain(doc, data, key, now):
    brain = doc.setdefault("brain", brain_default())
    kind = data.get("type")
    if kind == "settings":
        patch = data.get("config")
        if not isinstance(patch, dict) or set(patch)-set(brain["config"]):
            raise ValueError("unknown companion setting")
        brain["config"].update(patch); brain["nextCheckAt"] = 0
        if not brain["config"]["romance"]:
            for name in ["sweet", "jealous"]:
                brain["emotions"][name] = 0
    elif kind == "consume":
        if any(e["id"] == data.get("eventId") for e in brain["timeline"]) and data.get("eventId") not in brain["consumedIds"]:
            brain.update(consumed=data["eventId"], injection=clean(data.get("text", ""), 600), injectedAt=now)
            brain["consumedIds"] = (brain["consumedIds"] + [data["eventId"]])[-200:]
    elif kind == "mergeMeta":
        memory_id = ident(data.get("id"))
        if memory_id not in brain["memories"] and any(m["id"] == memory_id and m["text"] == data.get("expected") and not m["deleted"] for m in doc["memories"]):
            brain["memories"][memory_id] = json.loads(json.dumps(data["meta"]))
    elif kind == "decision":
        if now < brain["nextCheckAt"]:
            return
        chance, roll = number(data.get("chance", 0)), number(data.get("roll", 0))
        brain["nextCheckAt"] = now + brain["config"]["intervalMinutes"] * 60000
        brain["misses"] = 0 if roll < chance else min(1000, brain["misses"]+1)
        brain["decisions"] = (brain["decisions"] + [dict(at=now, reason=clean(data.get("reason", ""), 200), chance=chance, roll=roll, nextAt=brain["nextCheckAt"])])[-60:]
    elif kind == "log":
        brain["decisions"] = (brain["decisions"] + [dict(at=now, reason=clean(data.get("reason", ""), 200), chance=0, roll=0, nextAt=0)])[-60:]
    elif kind == "resetMood":
        doc["pet"].update(mood=brain["config"]["baseline"], moodReason="已重置到平稳心情", moodAt=now)
        doc["moodHistory"] = []
        brain.update(emotions={k: 0 for k in EMOTIONS}, emotionAt=now, event=None, timeline=[], consumed="", consumedIds=[], injection="")
    elif kind in {"editMemory", "undoMemory"}:
        memory_id = ident(data.get("id"))
        item = next((m for m in doc["memories"] if m["id"] == memory_id and not m["deleted"]), None)
        if not item or data.get("expected") != item["text"]:
            raise ValueError("memory changed; refresh before editing")
        meta = brain["memories"].setdefault(memory_id, meta_default())
        if kind == "undoMemory":
            if not meta["versions"]:
                raise ValueError("no previous version")
            item.update(meta["versions"].pop())
        else:
            for field in ["importance", "emotion", "tags", "key", "kind"]:
                if field in data:
                    meta[field] = data[field]
    else:
        raise ValueError("unknown companion operation")
    validate_brain(brain); sanitize_brain(brain)


def sync(root, request, now=None):
    now = int(time.time() * 1000) if now is None else now
    if set(request) != {"protocol", "namespace", "device", "claim", "seed", "operations"} or request["protocol"] != 1:
        raise ValueError("unsupported protocol")
    namespace, device = ident(request["namespace"]), ident(request["device"])
    if not isinstance(request["claim"], bool) or not isinstance(request["operations"], list) or len(request["operations"]) > 250:
        raise ValueError("invalid batch")
    root = Path(root)
    if root.is_symlink():
        raise ValueError("unexpected storage path")
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    path = root / (namespace + ".sqlite3")
    if path.is_symlink():
        raise ValueError("unexpected database path")
    conn = sqlite3.connect(path, timeout=8)
    os.chmod(path, 0o600)
    try:
        conn.execute("CREATE TABLE IF NOT EXISTS state(id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL, holder TEXT, lease INTEGER)")
        conn.execute("CREATE TABLE IF NOT EXISTS received(id TEXT PRIMARY KEY, digest TEXT NOT NULL)")
        conn.execute("BEGIN IMMEDIATE")
        row = conn.execute("SELECT body,holder,lease FROM state WHERE id=1").fetchone()
        doc, holder, lease = (validate(json.loads(row[0])), row[1], row[2]) if row else (initial(request["seed"], now), "", 0)
        advance(doc, now)
        interacted = False
        for op in request["operations"]:
            key = ident(op.get("id"))
            digest = hashlib.sha256(json.dumps(op, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
            old = conn.execute("SELECT digest FROM received WHERE id=?", (key,)).fetchone()
            if old:
                if old[0] != digest:
                    raise ValueError("operation identity conflict")
                continue
            if op.get("kind") == "social" and op["data"].get("type") == "reserve" and op["data"].get("manual") is True:
                holder, lease = device, now + 45000
            restricted = (op.get("kind") == "social" and op["data"].get("type") in {"reserve", "commit"}) or (op.get("kind") == "brain" and op["data"].get("type") == "decision")
            if not (restricted and (holder != device or lease <= now)):
                apply(doc, op, now)
            interacted = interacted or op.get("kind") == "care" or (op.get("kind") == "social" and op["data"].get("type") == "user")
            doc["revision"] += 1
            conn.execute("INSERT INTO received VALUES(?,?)", (key, digest))
        if request["claim"] and (holder == device or lease <= now or interacted):
            holder, lease = device, now + 45000
        while len(json.dumps(doc, ensure_ascii=False).encode()) > 4 * 1024 * 1024 and doc["conversations"]:
            doc["conversations"].pop(0)
        if len(json.dumps(doc, ensure_ascii=False).encode()) > 4 * 1024 * 1024:
            raise ValueError("shared document is full")
        validate(doc)
        conn.execute("INSERT OR REPLACE INTO state VALUES(1,?,?,?)", (json.dumps(doc, ensure_ascii=False), holder, lease))
        conn.commit()
        return {"document": doc, "acknowledged": [x["id"] for x in request["operations"]],
                "active": holder == device and lease > now, "serverTime": now}
    finally:
        conn.close()


if __name__ == "__main__":
    try:
        os.umask(0o077)
        raw = sys.stdin.buffer.read(LIMIT + 1)
        if len(raw) > LIMIT:
            raise ValueError("request too large")
        result = sync(Path.home() / ".local/share/penguinpal-sync/v1", json.loads(raw))
        sys.stdout.write(json.dumps(result, ensure_ascii=False))
    except Exception:
        sys.stderr.write("PenguinPal sync failed: check Python 3, account storage permissions and data format.\n")
        sys.exit(1)

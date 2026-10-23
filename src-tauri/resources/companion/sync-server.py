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
    if not isinstance(doc, dict) or set(doc) - {"social"} != {"schema", "petId", "revision", "updatedAt", "pet", "memories", "conversations", "moodHistory", "lastFeedAt", "lastPatAt"} or doc["schema"] != 1:
        raise ValueError("unsupported data format")
    ident(doc["petId"])
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
    pet["mood"] = 70 + (pet["mood"] - 70) * math.pow(0.5, hours / 12)
    doc["updatedAt"] = max(now, doc["updatedAt"])


def apply(doc, op, now):
    ident(op["id"])
    if set(op) != {"id", "kind", "data"} or not isinstance(op["data"], dict):
        raise ValueError("invalid operation")
    data, pet = op["data"], doc["pet"]
    if op["kind"] == "social":
        apply_social(doc, data, op["id"], now)
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
        if old:
            doc["memories"].remove(old)
        if len(doc["memories"]) >= 2000:
            raise ValueError("memory limit reached")
        doc["memories"].append({"id": key, "text": "" if data["deleted"] else text, "deleted": data["deleted"], "at": now})
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
            apply(doc, {"id": op["id"], "kind": "memory", "data": {k: item[k] for k in ["id", "text", "deleted"]}}, now)
        for item in imported["conversations"]:
            apply(doc, {"id": op["id"], "kind": "conversation", "data": item}, now)
        # Explicit restore keeps the existing shared identity; collections merge.
        doc["pet"] = imported["pet"].copy()
        doc["moodHistory"] = imported["moodHistory"][-200:]
    else:
        raise ValueError("unknown operation")


def social_default():
    return {"enabled": True, "proactive": False, "analyzed": [], "lastOutreachAt": 0, "lastResponseAt": 0,
            "unanswered": 0, "lastTopic": "", "reservation": "", "reservedUntil": 0, "day": 0, "sentToday": 0}


def can_send(social, now):
    return (social["proactive"] and social["unanswered"] < 2 and
            (social["day"] != now // 86400000 or social["sentToday"] < 4) and
            (social["lastOutreachAt"] == 0 or now - social["lastOutreachAt"] >= 45 * 60000 * (1 + social["unanswered"] * 2)) and
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
            memory_id = hashlib.sha256(("learned:" + kind + ":" + attribute).encode()).hexdigest()
            old = next((m for m in doc["memories"] if m["id"] == memory_id), None)
            if old and (old["deleted"] or old["at"] > source_at):
                continue
            text = "【" + labels[kind] + "】" + text + "\n依据：「" + quote + "」\n来源：" + source_id + " / " + session + " / " + str(source_at)
            if old:
                doc["memories"].remove(old)
            if len(doc["memories"]) >= 2000:
                raise ValueError("memory limit reached")
            doc["memories"].append({"id": memory_id, "text": text, "deleted": False, "at": source_at})
        delta = number(data.get("delta"), -3, 3)
        if not isinstance(delta, int):
            raise ValueError("invalid mood delta")
        if delta and not historical:
            reason, quote = clean(data.get("reason"), 160), clean(data.get("quote"), 160)
            if not reason.strip() or len(quote) < 2 or quote not in source:
                raise ValueError("missing mood evidence")
            apply(doc, {"id": key, "kind": "mood", "data": {"delta": delta, "reason": reason + "；依据：「" + quote + "」"}}, now)
        social["analyzed"] = (social["analyzed"] + [source_id])[-500:]
    elif kind == "reserve":
        token = ident(data.get("token"))
        if can_send(social, now):
            social["reservation"], social["reservedUntil"] = token, now + 180000
    elif kind == "release":
        if data.get("token") == social["reservation"]:
            social["reservation"], social["reservedUntil"] = "", 0
    elif kind == "commit":
        if not social["proactive"] or not social["reservation"] or data.get("token") != social["reservation"] or social["reservedUntil"] <= now:
            return
        text, topic = clean(data.get("text"), 1200), clean(data.get("topic"), 100)
        if not text.strip() or not topic.strip():
            raise ValueError("empty opener")
        if topic != social["lastTopic"]:
            social_message(doc, key, "assistant", text, now)
            if social["day"] != now // 86400000:
                social["day"], social["sentToday"] = now // 86400000, 0
            social["sentToday"] += 1
            social["unanswered"] += 1
            social["lastOutreachAt"], social["lastTopic"] = now, topic
        social["reservation"], social["reservedUntil"] = "", 0
    else:
        raise ValueError("invalid social operation")


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
            if not (op.get("kind") == "social" and op["data"].get("type") in {"reserve", "commit"} and (holder != device or lease <= now)):
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

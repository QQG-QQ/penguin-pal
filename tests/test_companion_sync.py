import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("sync_server", Path(__file__).parents[1] / "src-tauri/resources/companion/sync-server.py")
server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(server)
NOW = 1800000000000


def op(key, kind, **data):
    return {"id": key, "kind": kind, "data": data}


class SyncScenarios(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.root = Path(self.folder.name) / "private"

    def tearDown(self):
        self.folder.cleanup()

    def sync(self, device="A", operations=None, now=NOW, seed=None, namespace="pet"):
        return server.sync(self.root, {"protocol": 1, "namespace": namespace, "device": device, "claim": True,
                            "seed": seed or {}, "operations": operations or []}, now)

    def test_two_devices_keep_identity_and_retry_exactly_once(self):
        first = self.sync(seed={"energy": 40, "hunger": 80, "affection": 90})
        feed = op("feed-1", "care", action="feed")
        result = self.sync(operations=[feed])
        retry = self.sync(operations=[feed])
        other = self.sync("B")
        self.assertEqual(first["document"]["petId"], other["document"]["petId"])
        self.assertEqual(other["document"]["pet"]["hunger"], 62)
        self.assertEqual(result["document"], retry["document"])
        self.assertEqual(result["document"], other["document"])

    def test_conversation_memory_mood_and_restore(self):
        messages = [
            op("mem", "memory", id="favorite", text="喜欢安静陪伴", deleted=False),
            op("chat", "conversation", id="chat-message", session="conversation-1", role="user", text="明天提醒我喝水", at=NOW),
            op("mood", "mood", delta=5, reason="刚才被夸了")]
        result = self.sync(operations=messages)
        other = self.sync("B")
        self.assertEqual(result["document"], other["document"])
        self.assertEqual(other["document"]["pet"]["mood"], 75)
        restored = self.sync("B", [op("restore-1", "restore", document=result["document"])])
        self.assertEqual(len(restored["document"]["conversations"]), 1)

    def test_deletion_is_not_resurrected_by_offline_client_or_import(self):
        add = op("add", "memory", id="m", text="旧约定", deleted=False)
        original = self.sync(operations=[add])["document"]
        self.sync(operations=[op("delete", "memory", id="m", text="", deleted=True)])
        result = self.sync("B", [op("restore", "restore", document=original)])
        self.assertTrue(result["document"]["memories"][0]["deleted"])
        self.assertEqual(result["document"]["memories"][0]["text"], "")

    def test_failed_batch_and_identity_conflict_are_atomic(self):
        baseline = self.sync()["document"]
        with self.assertRaises(ValueError):
            self.sync(operations=[op("good", "mood", delta=5), op("bad", "execute", command="never")])
        self.assertEqual(self.sync()["document"], baseline)
        self.sync(operations=[op("same", "mood", delta=1)])
        with self.assertRaises(ValueError):
            self.sync(operations=[op("same", "mood", delta=2)])
        self.assertEqual(self.sync()["document"]["pet"]["mood"], 71)

    def test_retention_timestamp_and_no_reverse_clock_decay(self):
        for batch in range(3):
            self.sync(operations=[op("event-" + str(n), "conversation", id="msg-" + str(n), session="s",
                                    role="assistant", text=str(n), at=NOW - 1000 + n)
                                  for n in range(batch * 200, (batch + 1) * 200)])
        result = self.sync(now=NOW + 3600000)["document"]
        self.assertEqual(len(result["conversations"]), 500)
        self.assertEqual(result["conversations"][0]["text"], "100")
        backward = self.sync(now=NOW)["document"]
        self.assertEqual(result, backward)

    def test_active_device_lease_and_interaction_takeover(self):
        self.assertTrue(self.sync()["active"])
        self.assertFalse(self.sync("B")["active"])
        self.assertTrue(self.sync("B", [op("pat", "care", action="pat")])["active"])
        self.assertFalse(self.sync("A")["active"])
        self.assertTrue(self.sync("A", now=NOW + 46000)["active"])

    def test_invalid_paths_unknown_fields_and_secret_redaction(self):
        with self.assertRaises(ValueError):
            self.sync(namespace="../../other")
        result = self.sync(operations=[op("secret", "conversation", id="c", session="s",
                           role="user", text="password=example-secret", at=NOW)])
        self.assertNotIn("example-secret", json.dumps(result))
        document = result["document"]
        document["credentials"] = {"password": "not-supported"}
        with self.assertRaises(ValueError):
            self.sync(operations=[op("invalid", "restore", document=document)])

    def test_existing_seed_preserves_local_pet_and_data(self):
        source = server.initial({"energy": 12, "affection": 99}, NOW)
        source["memories"] = [{"id": "existing", "text": "已有记忆", "at": NOW, "deleted": False}]
        result = self.sync(seed=source)["document"]
        self.assertEqual(result["petId"], source["petId"])
        self.assertEqual(result["pet"]["energy"], 12)
        self.assertEqual(result["memories"], source["memories"])

    def test_bubble_history_stays_visible_when_local_pet_joins_existing_identity(self):
        local = server.initial({}, NOW)
        remote = self.sync()["document"]
        self.assertNotEqual(local["petId"], remote["petId"])
        server.apply(local, op("local-user", "social", type="user", text="连接服务器前的对话"), NOW)
        imported = self.sync("B", [op("merge-local-chat", "conversation", **local["conversations"][0]), op("shared-user", "social", type="user", text="连接服务器后的对话")])["document"]
        self.assertEqual(imported["conversations"][0]["session"], imported["conversations"][1]["session"])

    def test_social_learning_cross_device_replay_tombstone_and_atomic_evidence(self):
        self.sync(operations=[op("user-1", "social", type="user", text="我喜欢清淡的小鱼，谢谢你陪我")])
        data = dict(type="analysis", sourceId="user-1", memories=[dict(kind="preference", key="饮食", text="喜欢清淡的小鱼", quote="我喜欢清淡的小鱼")], delta=2, reason="受到感谢", quote="谢谢你陪我")
        first = self.sync(operations=[op("learn-A", "social", **data)])["document"]
        second = self.sync("B", [op("learn-B", "social", **data)])["document"]
        self.assertEqual(first["pet"], second["pet"])
        self.assertEqual(first["memories"], second["memories"])
        self.assertEqual(len(second["moodHistory"]), 1)
        self.sync(operations=[op("forget", "memory", id=first["memories"][0]["id"], text="", deleted=True), op("user-2", "social", type="user", text="我喜欢清淡的小鱼，谢谢你陪我")])
        data["sourceId"] = "user-2"
        data["quote"] = "没有说过"
        before = self.sync()["document"]
        with self.assertRaises(ValueError):
            self.sync(operations=[op("bad-evidence", "social", **data)])
        self.assertEqual(self.sync()["document"], before)
        data["quote"] = "谢谢你陪我"
        learned = self.sync("B", [op("retry-learning", "social", **data)])["document"]
        self.assertTrue(learned["memories"][0]["deleted"])
        restored = self.sync("B", [op("restore-social", "restore", document=first)])["document"]
        self.assertEqual(restored["social"], learned["social"])

    def test_social_active_device_reservation_expiry_busy_takeover_and_fixed_cooldown(self):
        self.sync(operations=[op("settings", "social", type="settings", enabled=True, proactive=True)])
        self.sync(operations=[op("reserve-A", "social", type="reserve", token="A")])
        other = self.sync("B", [op("reserve-B", "social", type="reserve", token="B")])
        self.assertEqual(other["document"]["social"]["reservation"], "A")
        other = self.sync("B", [op("wrong-commit", "social", type="commit", token="A", text="不应发出", topic="topic")])
        self.assertEqual(len(other["document"]["conversations"]), 0)
        first = self.sync(operations=[op("commit-A", "social", type="commit", token="A", text="小鱼喜欢什么口味？", topic="小鱼")])
        self.assertEqual(first["document"]["social"]["unanswered"], 1)
        self.assertFalse(server.can_send(first["document"]["social"], NOW + 45*60000 - 1))
        self.assertTrue(server.can_send(first["document"]["social"], NOW + 45*60000))
        takeover = self.sync("B", [op("response-B", "social", type="user", text="我回来啦")])
        self.assertTrue(takeover["active"])
        self.assertEqual(takeover["document"]["social"]["unanswered"], 0)
        # Replaying a previously acknowledged interaction cannot steal ownership.
        self.sync("A", [op("care-A", "care", action="pat")])
        self.assertFalse(self.sync("B", [op("response-B", "social", type="user", text="我回来啦")])["active"])
        later = NOW + 45*60000
        self.sync(now=later)
        self.sync(operations=[op("reserve-later", "social", type="reserve", token="later")], now=later)
        expired = self.sync(operations=[op("commit-expired", "social", type="commit", token="later", text="过时消息", topic="过时")], now=later+180001)
        self.assertEqual(len(expired["document"]["conversations"]), 2)

    def test_social_response_and_disable_cancel_and_daily_limits(self):
        doc = server.initial({}, NOW)
        def apply(key, **data): server.apply(doc, op(key, "social", **data), NOW)
        apply("settings", type="settings", enabled=True, proactive=True)
        apply("reserve", type="reserve", token="t")
        apply("user", type="user", text="等等")
        apply("commit", type="commit", token="t", text="已取消", topic="取消")
        self.assertEqual(len(doc["conversations"]), 1)
        apply("reserve2", type="reserve", token="t2")
        apply("off", type="settings", enabled=True, proactive=False)
        apply("commit2", type="commit", token="t2", text="已关闭", topic="关闭")
        self.assertEqual(len(doc["conversations"]), 1)
        state = server.social_default(); state["proactive"] = True
        for unanswered in [0, 1, 2, 1000]:
            state["unanswered"] = unanswered
            state["lastOutreachAt"] = NOW
            self.assertFalse(server.can_send(state, NOW + 45*60000 - 1))
            self.assertTrue(server.can_send(state, NOW + 45*60000))
        state["unanswered"] = 0; state["day"] = NOW//86400000; state["sentToday"] = 4
        self.assertFalse(server.can_send(state, NOW))
        self.assertTrue(server.can_send(state, NOW+86400000))

    def test_unanswered_outreach_continues_across_devices_until_daily_limit(self):
        self.sync(operations=[op("settings", "social", type="settings", enabled=True, proactive=True)])
        for n, device in enumerate(["A", "B", "A", "B"]):
            at = NOW + n * 45 * 60000
            self.sync(device, now=at)
            reserved = self.sync(device, [op(f"reserve-{n}", "social", type="reserve", token=f"token-{n}")], now=at)
            self.assertEqual(reserved["document"]["social"]["reservation"], f"token-{n}")
            committed = self.sync(device, [op(f"commit-{n}", "social", type="commit", token=f"token-{n}", text=f"聊聊近况{n}", topic=f"topic-{n}")], now=at)
            self.assertEqual(committed["document"]["social"]["sentToday"], n+1)
            self.assertEqual(committed["document"]["social"]["unanswered"], n+1)
        at = NOW + 4 * 45 * 60000
        self.sync("A", now=at)
        blocked = self.sync("A", [op("reserve-fifth", "social", type="reserve", token="fifth")], now=at)
        self.assertEqual(blocked["document"]["social"]["reservation"], "")
        self.assertEqual(len(blocked["document"]["conversations"]), 4)

    def test_history_after_eviction_syncs_evidence_time_and_ignores_old_emotions(self):
        source = {"id": "old-source", "session": "outside-project", "role": "user", "text": "喜欢清淡小鱼，谢谢你陪我", "at": NOW-30*86400000}
        data = dict(type="history", sourceId=source["id"], source=source,
                    memories=[dict(kind="preference", key="饮食", text="喜欢清淡小鱼", quote="喜欢清淡小鱼")],
                    delta=3, reason="受到感谢", quote="谢谢你陪我")
        first = self.sync(operations=[op("history", "social", **data)])["document"]
        other = self.sync("B")["document"]
        self.assertEqual(first, other)
        self.assertEqual(other["pet"]["mood"], 70)
        self.assertEqual(other["moodHistory"], [])
        self.assertEqual(other["conversations"], [])
        self.assertEqual(other["memories"][0]["at"], source["at"])
        self.assertIn("来源：old-source / outside-project", other["memories"][0]["text"])
        data["sourceId"] = "older-source"; data["source"]["id"] = "older-source"; data["source"]["at"] -= 86400000
        data["memories"][0]["text"] = "过期喜好不应覆盖"
        older = self.sync("B", [op("older", "social", **data)])["document"]
        self.assertEqual(older["memories"], other["memories"])
        memory_id = other["memories"][0]["id"]
        self.sync(operations=[op("forget-history", "memory", id=memory_id, text="", deleted=True)])
        data["sourceId"] = "replayed-source"; data["source"]["id"] = "replayed-source"
        tombstone = self.sync("B", [op("replay-history", "social", **data)])["document"]
        self.assertTrue(tombstone["memories"][0]["deleted"])

    def test_history_invalid_source_or_quote_rolls_back_batch(self):
        self.sync()
        source = dict(id="source", session="thread", role="assistant", text="喜欢小鱼", at=1)
        data = dict(type="history", sourceId="source", source=source,
                    memories=[dict(kind="preference", key="饮食", text="喜欢小鱼", quote="喜欢小鱼")], delta=0)
        with self.assertRaises(ValueError):
            self.sync(operations=[op("invalid-history", "social", **data)])
        source["role"] = "user"; data["memories"][0]["quote"] = "不存在的原话"
        with self.assertRaises(ValueError):
            self.sync(operations=[op("invented-history", "social", **data)])
        self.assertEqual(self.sync()["document"]["memories"], [])


class BrainScenarios(unittest.TestCase):
    setUp = SyncScenarios.setUp
    tearDown = SyncScenarios.tearDown
    sync = SyncScenarios.sync
    def test_configuration_and_emotions_follow_another_device_and_survive_restore(self):
        self.sync(operations=[op("config", "brain", type="settings", config={"probability": 100, "moodHalfHours": 4})])
        self.sync(operations=[op("user", "social", type="user", text="谢谢你陪我")])
        data = dict(type="analysis", sourceId="user", memories=[], delta=2, dims={"happy": 4, "jealous": 6},
                    confidence=85, reason="被感谢", quote="谢谢你陪我")
        doc = self.sync(operations=[op("analysis", "social", **data)])["document"]
        other = self.sync("B")["document"]
        self.assertEqual(doc, other)
        self.assertEqual(other["brain"]["emotions"]["happy"], 4)
        self.assertEqual(other["brain"]["emotions"]["jealous"], 0)
        self.assertEqual(other["brain"]["event"]["source"], "user")
        self.sync("B", [op("consumed", "brain", type="consume", eventId="analysis", text="被感谢")])
        self.assertEqual(self.sync()["document"]["brain"]["consumed"], "analysis")
        later = self.sync(now=NOW+90*60000)["document"]
        self.assertAlmostEqual(later["brain"]["emotions"]["happy"], 2)

    def test_memory_versions_undo_and_tombstones_erase_version_content(self):
        def learn(n, text):
            self.sync(operations=[op("u"+str(n), "social", type="user", text=text)])
            return self.sync(operations=[op("a"+str(n), "social", type="analysis", sourceId="u"+str(n),
                memories=[dict(kind="preference", key="饮食", text=text, quote=text, importance=.9, emotion=.2, tags=["小鱼"])], delta=0)])["document"]
        first = learn(1, "我喜欢清淡小鱼")
        second = learn(2, "我现在喜欢烤小鱼")
        key = second["memories"][0]["id"]
        self.assertEqual(len(second["brain"]["memories"][key]["versions"]), 1)
        undone = self.sync("B", [op("undo", "brain", type="undoMemory", id=key, expected=second["memories"][0]["text"])])["document"]
        self.assertEqual(undone["memories"][0]["text"], first["memories"][0]["text"])
        self.sync(operations=[op("forget-new", "memory", id=key, deleted=True, text="")])
        forgotten = self.sync("B", [op("restore-before-delete", "restore", document=second)])["document"]
        self.assertTrue(forgotten["memories"][0]["deleted"])
        self.assertNotIn(key, forgotten["brain"]["memories"])

    def test_recent_events_do_not_overwrite_one_another_and_history_ignores_daily_cap(self):
        self.sync(operations=[op("cap", "brain", type="settings", config={"dailyMemories": 1})])
        for n in range(3):
            source = dict(id=f"old{n}", role="user", session="archive", text=f"今天一起看海{n}", at=NOW-n*86400000)
            self.sync(operations=[op(f"h{n}", "social", type="history", sourceId=source["id"], source=source,
                memories=[dict(kind="recent", key="看海", text=source["text"], quote=source["text"])], delta=3, confidence=90, dims={"happy": 5})])
        doc = self.sync("B")["document"]
        self.assertEqual(len(doc["memories"]), 3)
        self.assertEqual(doc["pet"]["mood"], 70)
        self.assertEqual(doc["brain"]["timeline"], [])

    def test_invalid_config_or_stale_edit_rolls_back_whole_batch(self):
        before = self.sync()["document"]
        with self.assertRaises(ValueError):
            self.sync(operations=[op("good-setting", "brain", type="settings", config={"probability": 99}),
                                  op("bad-setting", "brain", type="settings", config={"intervalMinutes": 0})])
        self.assertEqual(self.sync()["document"], before)
        self.sync(operations=[op("m-new", "memory", id="memory", text="喜欢看海", deleted=False)])
        with self.assertRaises(ValueError):
            self.sync("B", [op("stale", "brain", type="editMemory", id="memory", expected="旧内容", importance=1)])

    def test_manual_send_can_take_over_but_still_reserves_and_can_be_cancelled(self):
        self.sync()
        reserve = self.sync("B", [op("manual-reserve", "social", type="reserve", token="manual", manual=True)])
        self.assertTrue(reserve["active"])
        committed = self.sync("B", [op("manual-commit", "social", type="commit", token="manual", text="一起看看海吗", topic="海",
            selfMood=dict(delta=1, dims={"calm": 2}, reason="主动关心", confidence=80))])["document"]
        self.assertEqual(len(committed["conversations"]), 1)
        self.assertEqual(committed["brain"]["event"]["source"], "speak-up")
        self.sync("B", [op("manual-reserve2", "social", type="reserve", token="manual2", manual=True)])
        self.sync("A", [op("cancel-user", "social", type="user", text="先等等")])
        cancelled = self.sync("B", [op("manual-commit2", "social", type="commit", token="manual2", text="不应出现", topic="取消")])["document"]
        self.assertEqual(len(cancelled["conversations"]), 2)

    def test_shared_random_clock_and_misses_are_idempotent_across_devices(self):
        self.sync(operations=[op("random-config", "brain", type="settings", config={"intervalMinutes": 1})])
        decision = op("decision1", "brain", type="decision", chance=20, roll=80, reason="随机未命中")
        first = self.sync(operations=[decision])["document"]
        self.assertEqual(first["brain"]["misses"], 1)
        self.assertEqual(first, self.sync(operations=[decision])["document"])
        self.sync("B", [op("user-takeover", "social", type="user", text="我回来了")])
        early = self.sync("B", [op("too-early", "brain", type="decision", chance=20, roll=80, reason="还没到")])["document"]
        self.assertEqual(early["brain"]["misses"], 1)
        self.sync("B", now=NOW+60000)
        after = self.sync("B", [op("decision2", "brain", type="decision", chance=40, roll=10, reason="命中")], now=NOW+60000)["document"]
        self.assertEqual(after["brain"]["misses"], 0)
        self.assertEqual(len(after["brain"]["decisions"]), 2)

    def test_late_event_acknowledgment_does_not_unconsume_newer_events(self):
        for n in [1, 2]:
            self.sync(operations=[op(f"user-{n}", "social", type="user", text="谢谢你陪我")])
            self.sync(operations=[op(f"mood-{n}", "social", type="analysis", sourceId=f"user-{n}", memories=[],
                delta=1, confidence=90, dims={"happy": 1}, reason="被感谢", quote="谢谢你陪我")])
        self.sync(operations=[op("ack-new", "brain", type="consume", eventId="mood-2", text="新的感受")])
        other = self.sync("B", [op("ack-old", "brain", type="consume", eventId="mood-1", text="较早感受")])["document"]
        self.assertEqual(set(other["brain"]["consumedIds"]), {"mood-1", "mood-2"})
        self.assertEqual(other["brain"]["event"]["id"], "mood-2")

    def test_reconfirmed_fact_rejects_older_changes_and_suffix_changes_keep_versions(self):
        def learn(n, text, at):
            source = dict(id=f"fact-source-{n}", role="user", session="archive", text=text, at=at)
            return self.sync(operations=[op(f"fact-learn-{n}", "social", type="history", sourceId=source["id"], source=source,
                memories=[dict(kind="preference", key="food.flavor", text=text, quote=text)], delta=0)])["document"]
        first = learn(1, "喜欢清淡小鱼", NOW-30*86400000)
        confirmed = learn(2, "喜欢清淡小鱼", NOW-86400000)
        stale = learn(3, "以前喜欢烤小鱼", NOW-2*86400000)
        self.assertEqual(stale["memories"], confirmed["memories"])
        self.assertEqual(stale["brain"]["memories"], confirmed["brain"]["memories"])
        changed = learn(4, "小鱼", NOW)
        key = changed["memories"][0]["id"]
        self.assertEqual(changed["brain"]["memories"][key]["versions"], [dict(text=first["memories"][0]["text"], at=first["memories"][0]["at"])])

    def test_repeated_event_keeps_original_date_and_strengthens_recall(self):
        old_at = NOW-30*86400000
        for n, at in [(1, old_at), (2, NOW)]:
            source = dict(id=f"event-source-{n}", role="user", session="archive", text="我们去海边散步", at=at)
            self.sync(operations=[op(f"event-learn-{n}", "social", type="history", sourceId=source["id"], source=source,
                memories=[dict(kind="recent", key="海边散步事件", text="一起去海边散步", quote="去海边散步", importance=0)], delta=0)])
        doc = self.sync("B")["document"]
        self.assertEqual(len(doc["memories"]), 1)
        item = doc["memories"][0]
        self.assertEqual(item["at"], old_at)
        self.assertEqual(doc["brain"]["memories"][item["id"]]["reinforcedAt"], NOW)
        self.assertAlmostEqual(doc["brain"]["memories"][item["id"]]["strength"], .6)


if __name__ == "__main__":
    unittest.main()

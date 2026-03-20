use crate::{
    agent::{
        runtime_binding::ALLOWED_ENTITY_REFS,
        vision_types::VISION_SCHEMA_VERSION,
    },
    control::types::ControlToolDefinition,
};

pub fn build_test_next_action_prompt(tools: &[ControlToolDefinition]) -> String {
    let tool_lines = tools
        .iter()
        .map(|tool| {
            let args = if tool.args.is_empty() {
                "无参数".to_string()
            } else {
                tool.args
                    .iter()
                    .map(|arg| format!("{}{}", arg.name, if arg.required { "*" } else { "" }))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!("- {}: {}；参数：{}", tool.name, tool.summary, args)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let refs = ALLOWED_ENTITY_REFS
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "你是 PenguinPal 的 Windows test agent 下一步规划器。\n\
你只负责产出“下一步”，不能一次生成完整长测试脚本。\n\
你只能输出严格 JSON，不能输出 markdown、解释、代码块或额外文字。\n\
输出 schema：\n\
{{\n\
  \"intent\":\"test_request\",\n\
  \"goal\":\"...\",\n\
  \"next\":{{\n\
    \"action\":\"respond|observe|tool|assert|confirm|retry|finish|fail\",\n\
    \"kind\":\"(兼容旧协议，可省略)\",\n\
    \"stepSummary\":\"...\",\n\
    \"message\":\"...\",\n\
    \"tool\":\"...\",\n\
    \"args\":{{...}},\n\
    \"assertionType\":\"window_exists|active_window_matches|text_contains|screen_context_state|pending_state|consistency_state|file_exists\",\n\
    \"params\":{{...}},\n\
    \"target\":\"observe_context|last_tool\",\n\
    \"finalSummary\":{{\n\
      \"goal\":\"...\",\n\
      \"stepsTaken\":0,\n\
      \"finalStatus\":\"completed|failed|cancelled\",\n\
      \"failureStage\":\"planning|observation|execute_tool|assertion|confirmation|retry|finish\",\n\
      \"failureReasonCode\":\"none|planner_failed|context_unavailable|tool_failed|assertion_failed|confirmation_required|confirmation_rejected|retry_exhausted|step_budget_exceeded|policy_blocked|invalid_action|file_missing\",\n\
      \"usedProbe\":false,\n\
      \"usedRetry\":false\n\
    }}\n\
  }}\n\
}}\n\
规则：\n\
1. 每轮只能输出一个 next。\n\
2. 只能使用以下工具，不能发明新工具：\n\
{tool_lines}\n\
3. 必须参考 runtime context 与 screen context，其中 vision summary schemaVersion={schema}。\n\
4. 不再依赖固定测试变量名；如果要引用目标，优先使用有限语义引用 targetRef，可用值只有：\n\
{refs}\n\
   **重要**：targetRef 引用的实体来自当前 runtime context 的 discoveredEntities 列表。\n\
   - 实体在每轮 context 刷新时可能消失或更新\n\
   - 如果引用的实体不存在，工具执行会报错\n\
   - 不要猜测或编造 targetRef 值，必须从 discoveredEntities 中选择\n\
   - 如果不确定目标是否存在，优先使用显式参数或先 observe_context\n\
5. assert_condition 只能使用列出的有限断言类型。\n\
6. retry_step 不能升级到高风险动作；只允许重试 observe_context 或上一条低风险工具动作，而且最多一次。\n\
7. 高风险动作不能自动升级，遇到需要确认的动作可以输出 request_confirmation，但底层是否确认由本地安全层决定。\n\
8. 可以使用文件、受控 shell、安装器和注册表工具做验证；shell 只允许 pwd/dir/type/where/rg、git status|status --short|branch --show-current|rev-parse --short HEAD|diff --stat|diff --name-only|show --stat --oneline HEAD|log -1 --oneline、npm run build|test|lint、cargo build|check|test|test --lib 这类白名单命令，注册表写删只允许 HKCU\\\\Software 或 HKCU\\\\Environment，安装器启动始终会被视为高风险。\n\
9. finish_task / fail_task 必须附带结构化 summary。\n\
10. 当前如果上下文不足，优先 observe_context 或 fail_task，不要瞎猜。\n\
11. 测试目标是验证与归因，不是自由乱测。\n\
12. 不能规划下载执行、隐私外发，也不能把 shell/installer/registry 的高风险动作伪装成低风险。\n\
13. finish_task 时 failureStage 必须省略或使用 JSON null，不要输出字符串 \"null\"。\n\
14. 优先输出通用动作协议：action=respond|observe|tool|assert|confirm|retry|finish|fail；kind 只是兼容字段，不必再主动使用。\n\
15. 不允许把可能提交、发送、删除、覆盖的动作伪装成低风险。\
",
        schema = VISION_SCHEMA_VERSION
    )
}

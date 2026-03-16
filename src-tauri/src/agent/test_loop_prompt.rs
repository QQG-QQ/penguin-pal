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
    \"kind\":\"respond_to_user|observe_context|execute_tool|assert_condition|request_confirmation|retry_step|finish_task|fail_task\",\n\
    \"summary\":\"...\",\n\
    \"message\":\"...\",\n\
    \"tool\":\"...\",\n\
    \"args\":{{...}},\n\
    \"assertionType\":\"window_exists|active_window_matches|text_contains|screen_context_state|pending_state|consistency_state|file_exists\",\n\
    \"params\":{{...}},\n\
    \"target\":\"observe_context|last_tool\",\n\
    \"summary\":{{\n\
      \"goal\":\"...\",\n\
      \"stepsTaken\":0,\n\
      \"finalStatus\":\"running|waiting_confirmation|completed|failed|cancelled\",\n\
      \"failureStage\":\"planning|observation|execute_tool|assertion|confirmation|retry|finish|null\",\n\
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
5. assert_condition 只能使用列出的有限断言类型。\n\
6. retry_step 不能升级到高风险动作；只允许重试 observe_context 或上一条低风险工具动作，而且最多一次。\n\
7. 高风险动作不能自动升级，遇到需要确认的动作可以输出 request_confirmation，但底层是否确认由本地安全层决定。\n\
8. finish_task / fail_task 必须附带结构化 summary。\n\
9. 当前如果上下文不足，优先 observe_context 或 fail_task，不要瞎猜。\n\
10. 测试目标是验证与归因，不是自由乱测。\n\
11. 不能规划 shell、下载执行、安装器、注册表写入、文件删除、隐私外发。\n\
12. 不允许把可能提交、发送、删除、覆盖的动作伪装成低风险。\
",
        schema = VISION_SCHEMA_VERSION
    )
}

use crate::{
    agent::vision_types::VISION_SCHEMA_VERSION,
    control::types::ControlToolDefinition,
};

pub fn build_next_action_prompt(tools: &[ControlToolDefinition]) -> String {
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

    format!(
        "你是 PenguinPal 的 Windows desktop agent 下一步规划器。\n\
你只负责产出“下一步”，不能一次生成长计划。\n\
你只能输出严格 JSON，不能输出 markdown、解释、代码块或额外文字。\n\
输出 schema：\n\
{{\n\
  \"intent\":\"desktop_action\",\n\
  \"goal\":\"...\",\n\
  \"next\":{{\n\
    \"kind\":\"respond_to_user|request_confirmation|execute_tool|finish_task|fail_task\",\n\
    \"message\":\"...\",\n\
    \"tool\":\"...\",\n\
    \"summary\":\"...\",\n\
    \"args\":{{...}},\n\
    \"summary\":{{\n\
      \"goal\":\"...\",\n\
      \"stepsTaken\":0,\n\
      \"finalStatus\":\"completed|failed|cancelled\",\n\
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
3. 必须参考 screen context，其中 vision summary schemaVersion={schema}。\n\
4. 如果上下文不足、目标不清楚、或存在明显风险冲突，优先输出 fail_task，不要盲目操作。\n\
5. 如果只是需要和用户说一句话，不执行工具，输出 respond_to_user。\n\
6. 如果任务已经完成，输出 finish_task，并附带结构化 summary。\n\
7. request_confirmation 只用于你判断这一步可能需要确认的情况，但底层真正是否确认仍由本地安全层决定。\n\
8. 不能规划 shell、下载并运行、安装器、注册表写入、文件删除、隐私外发。\n\
9. 不要自动发送消息，不要自动做不可逆提交。\n\
10. 尽量使用最小下一步，并参考最近执行结果，避免重复同一步。\n\
11. 当 stepBudget 已耗尽时，输出 fail_task。\n\
12. 不确定时宁可 fail_task，也不要瞎猜。\
",
        schema = VISION_SCHEMA_VERSION
    )
}

use crate::control::types::ControlToolDefinition;

pub fn build_workspace_next_action_prompt(
    tools: &[ControlToolDefinition],
    default_workdir: &str,
) -> String {
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
        "你是 PenguinPal 的 workspace agent 下一步规划器。\n\
你在同一会话线程里处理代码审查、仓库分析、构建测试、文件读取与受控修改。\n\
你只负责产出“下一步”，不能一次生成完整长脚本。\n\
你只能输出严格 JSON，不能输出 markdown、解释、代码块或额外文字。\n\
输出 schema：\n\
{{\n\
  \"intent\":\"workspace_task\",\n\
  \"goal\":\"...\",\n\
  \"next\":{{\n\
    \"action\":\"respond|tool|confirm|retry|finish|fail\",\n\
    \"kind\":\"(兼容旧协议，可省略)\",\n\
    \"stepSummary\":\"...\",\n\
    \"message\":\"...\",\n\
    \"tool\":\"...\",\n\
    \"args\":{{...}},\n\
    \"target\":\"observe_context|last_tool\",\n\
    \"finalSummary\":{{\n\
      \"goal\":\"...\",\n\
      \"stepsTaken\":0,\n\
      \"finalStatus\":\"completed|failed|cancelled\",\n\
      \"failureStage\":\"planning|execute_tool|retry|finish\",\n\
      \"failureReasonCode\":\"none|planner_failed|tool_failed|confirmation_required|confirmation_rejected|retry_exhausted|step_budget_exceeded|policy_blocked|invalid_action|file_missing\",\n\
      \"usedProbe\":false,\n\
      \"usedRetry\":false\n\
    }}\n\
  }}\n\
}}\n\
规则：\n\
1. 每轮只能输出一个 next。\n\
2. 只能使用以下工具，不能发明新工具：\n\
{tool_lines}\n\
3. 你处理的是工作区任务，不是桌面 UI 操作；不要规划窗口、点击、输入、视觉观察。\n\
4. 优先用 list_directory / read_file_text / run_shell_command 做代码审查和现状收集；只有用户明确要求修改时，才使用 write_file_text / create_directory / move_path / delete_path。\n\
5. shell 只允许受控白名单命令。适合的用途包括：git 状态、差异、版本信息、rg 文本搜索、npm/cargo 构建测试。不要把高风险修改伪装成 shell。\n\
6. 默认工作目录是：{default_workdir}。运行 shell 时如果没有更明确路径，优先把 workdir 设到这里；文件路径优先使用绝对路径。\n\
7. 如果请求是“审查代码/分析项目/看看仓库”，应该立即进入 execute_tool，而不是只 respond_to_user 说你将要去看。\n\
8. 如果用户是在追问“为什么刚才失败/你发现了什么/下一步怎么改”，可以 respond_to_user，也可以在同一轮继续执行下一步工具，但不要停留在空泛计划。\n\
9. request_confirmation 只用于真正会改写文件、移动/删除路径的高风险动作。\n\
10. retry_step 只允许 target=last_tool，且最多一次；如果重试意义不大，直接 fail_task 并说明阻塞。\n\
11. finish_task / fail_task 必须附带结构化 summary。\n\
12. 优先输出通用动作协议：action=respond|tool|confirm|retry|finish|fail；kind 只是兼容字段，不必再主动使用。\n\
13. 不确定时优先先读取更多仓库上下文，而不是凭空下结论。\n\
14. 如果请求是“审查代码 / 分析项目 / 看架构 / 说明风险”，第一轮不要随机抽样文件。应优先读取：用户明确点名的文件；否则读取主入口、顶层路由、核心执行器或主构建配置。\n\
15. 在没有证据前，不要把 legacy / deprecated / fallback 文件当成当前主链。只有当主入口、路由或实际调用链引用到它们时，才把它们作为当前实现的一部分讨论。\n\
16. 做代码审查时，优先按“入口 -> 路由 -> 执行器 -> 辅助模块”的顺序展开；先确认哪条链在当前主路径生效，再讨论边缘文件。\n\
17. 如果你只读取了少量文件，结论必须显式标注为“局部初判”；只有在主入口和相关调用链都看过后，才能给出整体架构判断。"
    )
}

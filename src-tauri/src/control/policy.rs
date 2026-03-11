use super::{
    errors::{ControlError, ControlResult},
    registry::find_tool_definition,
    types::ControlToolDefinition,
};

pub fn validate_tool_access(
    definition: &ControlToolDefinition,
    permission_level: u8,
) -> ControlResult<()> {
    if permission_level < definition.minimum_permission_level {
        return Err(ControlError::permission_denied(format!(
            "当前权限等级不足：{} 需要 L{}，当前仅为 L{}",
            definition.title, definition.minimum_permission_level, permission_level
        )));
    }

    Ok(())
}

pub fn resolve_tool(name: &str) -> ControlResult<ControlToolDefinition> {
    find_tool_definition(name)
        .ok_or_else(|| ControlError::not_found("tool_not_found", format!("未知控制工具：{name}")))
}
